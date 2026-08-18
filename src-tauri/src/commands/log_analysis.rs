// ── LOG PATTERN ANALYSIS — Structured parsing & error clustering (P0 Feature 2) ─
//
// Parses log lines from read_log_tail / read_remote_log_* output and extracts:
//   1. Structured fields (timestamp, level, source, message) from common formats
//   2. Error/warning clusters — groups similar error messages by pattern
//   3. Frequency analysis — which error patterns appear most often
//   4. Timeline — error rate per hour/minute for the analyzed window
//
// Supported formats (auto-detected):
//   - Syslog: "May 18 12:34:56 hostname service[pid]: message"
//   - JSON lines: {"timestamp":"...","level":"...","message":"..."}
//   - Windows Event XML: <Event><System><TimeCreated .../></System>...</Event>
//   - Generic: lines starting with ISO-8601 timestamp or [LEVEL] prefix
//   - Plain: everything else — classified by keyword heuristics

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogAnalysisResult {
    pub total_lines: usize,
    pub parsed_lines: usize,
    pub levels: HashMap<String, usize>,        // INFO -> 42, ERROR -> 7, ...
    pub error_clusters: Vec<ErrorCluster>,      // grouped similar errors
    pub timeline: Vec<TimelineBucket>,          // errors per time bucket
    pub top_sources: Vec<(String, usize)>,      // top 10 log sources
    pub format_detected: String,                // 'syslog' | 'json' | 'generic' | 'plain'
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCluster {
    pub pattern: String,       // normalized pattern (numbers/IDs replaced with placeholders)
    pub count: usize,
    pub first_seen: String,    // earliest timestamp in the cluster
    pub last_seen: String,     // latest timestamp
    pub sample: String,        // one representative raw line
    pub level: String,         // ERROR, WARNING, CRITICAL, etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineBucket {
    pub timestamp: String,     // ISO-8601 truncated to hour or minute
    pub error_count: usize,
    pub warning_count: usize,
    pub total_count: usize,
}

#[derive(Debug, Clone)]
struct ParsedLine {
    timestamp: String,
    level: String,
    source: String,
    message: String,
    raw: String,
}

/// Analyze an array of log lines and return structured patterns, clusters, and timeline.
/// The frontend typically calls read_log_tail / read_remote_log_* first, then passes
/// the resulting Vec<String> here for analysis.
#[tauri::command]
pub async fn analyze_log_patterns(
    lines: Vec<String>,
    bucket_minutes: Option<usize>,
) -> Result<LogAnalysisResult, String> {
    let bucket_mins = bucket_minutes.unwrap_or(60).clamp(1, 1440);

    // Parse on a blocking thread — can be CPU-intensive for large logs
    tokio::task::spawn_blocking(move || {
        analyze_lines(&lines, bucket_mins)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

fn analyze_lines(lines: &[String], bucket_minutes: usize) -> Result<LogAnalysisResult, String> {
    if lines.is_empty() {
        return Ok(LogAnalysisResult {
            total_lines: 0,
            parsed_lines: 0,
            levels: HashMap::new(),
            error_clusters: vec![],
            timeline: vec![],
            top_sources: vec![],
            format_detected: "empty".into(),
        });
    }

    // Detect format from first non-empty lines
    let format = detect_format(lines);

    // Parse all lines
    let parsed: Vec<ParsedLine> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| parse_line(l, &format))
        .collect();

    let parsed_count = parsed.iter().filter(|p| !p.level.is_empty() || !p.timestamp.is_empty()).count();

    // Level counts
    let mut levels: HashMap<String, usize> = HashMap::new();
    for p in &parsed {
        if !p.level.is_empty() {
            *levels.entry(p.level.clone()).or_insert(0) += 1;
        }
    }

    // Source counts
    let mut source_counts: HashMap<String, usize> = HashMap::new();
    for p in &parsed {
        if !p.source.is_empty() {
            *source_counts.entry(p.source.clone()).or_insert(0) += 1;
        }
    }
    let mut top_sources: Vec<(String, usize)> = source_counts.into_iter().collect();
    top_sources.sort_by_key(|b| std::cmp::Reverse(b.1));
    top_sources.truncate(10);

    // Error clustering — group by normalized pattern
    let error_lines: Vec<&ParsedLine> = parsed
        .iter()
        .filter(|p| matches!(p.level.as_str(), "ERROR" | "CRITICAL" | "FATAL" | "WARN" | "WARNING"))
        .collect();

    let mut cluster_map: HashMap<String, (usize, String, String, String, String)> = HashMap::new();
    for el in &error_lines {
        let pattern = normalize_pattern(&el.message);
        let entry = cluster_map
            .entry(pattern.clone())
            .or_insert((0, el.timestamp.clone(), el.timestamp.clone(), el.raw.clone(), el.level.clone()));
        entry.0 += 1;
        if !el.timestamp.is_empty() {
            if el.timestamp < entry.1 || entry.1.is_empty() {
                entry.1 = el.timestamp.clone();
            }
            if el.timestamp > entry.2 {
                entry.2 = el.timestamp.clone();
            }
        }
    }

    let mut error_clusters: Vec<ErrorCluster> = cluster_map
        .into_iter()
        .map(|(pattern, (count, first, last, sample, level))| ErrorCluster {
            pattern,
            count,
            first_seen: first,
            last_seen: last,
            sample,
            level,
        })
        .collect();
    error_clusters.sort_by_key(|b| std::cmp::Reverse(b.count));
    error_clusters.truncate(50); // Top 50 clusters

    // Timeline buckets
    let timeline = build_timeline(&parsed, bucket_minutes);

    Ok(LogAnalysisResult {
        total_lines: lines.len(),
        parsed_lines: parsed_count,
        levels,
        error_clusters,
        timeline,
        top_sources,
        format_detected: format,
    })
}

/// Detect the log format from the first few non-empty lines.
fn detect_format(lines: &[String]) -> String {
    let sample: Vec<&str> = lines.iter().filter(|l| !l.trim().is_empty()).take(10).map(|s| s.as_str()).collect();

    if sample.is_empty() {
        return "plain".into();
    }

    // JSON lines: starts with { and is valid JSON
    let json_count = sample.iter().filter(|l| l.starts_with('{') && serde_json::from_str::<serde_json::Value>(l).is_ok()).count();
    if json_count > sample.len() / 2 {
        return "json".into();
    }

    // Syslog: "Mon DD HH:MM:SS hostname"
    let syslog_re = regex::Regex::new(r"^[A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\s+\S+").unwrap();
    let syslog_count = sample.iter().filter(|l| syslog_re.is_match(l)).count();
    if syslog_count > sample.len() / 2 {
        return "syslog".into();
    }

    // Windows Event XML
    if sample.iter().any(|l| l.contains("<Event xmlns=") || l.contains("<TimeCreated SystemTime=")) {
        return "event_xml".into();
    }

    // Generic: starts with ISO-8601-like timestamp or [LEVEL]
    let generic_re = regex::Regex::new(r"^(\d{4}-\d{2}-\d{2}[T ]|^\[\w+\])").unwrap();
    let generic_count = sample.iter().filter(|l| generic_re.is_match(l)).count();
    if generic_count > sample.len() / 2 {
        return "generic".into();
    }

    "plain".into()
}

/// Parse a single log line based on detected format.
fn parse_line(line: &str, format: &str) -> ParsedLine {
    match format {
        "json" => parse_json_line(line),
        "syslog" => parse_syslog_line(line),
        "generic" => parse_generic_line(line),
        _ => parse_plain_line(line),
    }
}

fn parse_json_line(line: &str) -> ParsedLine {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
        let ts = v.get("timestamp")
            .or_else(|| v.get("time"))
            .or_else(|| v.get("ts"))
            .or_else(|| v.get("@timestamp"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        let level = v.get("level")
            .or_else(|| v.get("severity"))
            .or_else(|| v.get("loglevel"))
            .and_then(|l| l.as_str())
            .unwrap_or("")
            .to_uppercase();

        let source = v.get("source")
            .or_else(|| v.get("logger"))
            .or_else(|| v.get("service"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();

        let message = v.get("message")
            .or_else(|| v.get("msg"))
            .or_else(|| v.get("error"))
            .and_then(|m| m.as_str())
            .unwrap_or(line)
            .to_string();

        ParsedLine { timestamp: ts, level, source, message, raw: line.to_string() }
    } else {
        parse_plain_line(line)
    }
}

fn parse_syslog_line(line: &str) -> ParsedLine {
    // "May 18 12:34:56 hostname service[pid]: message"
    let re = regex::Regex::new(
        r"^([A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})\s+(\S+)\s+(\S+?)(?:\[\d+\])?:\s*(.*)"
    ).unwrap();

    if let Some(caps) = re.captures(line) {
        let message = caps.get(4).map(|m| m.as_str()).unwrap_or("").to_string();
        let level = detect_level_from_text(&message);

        ParsedLine {
            timestamp: caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
            level,
            source: caps.get(3).map(|m| m.as_str()).unwrap_or("").to_string(),
            message,
            raw: line.to_string(),
        }
    } else {
        parse_plain_line(line)
    }
}

fn parse_generic_line(line: &str) -> ParsedLine {
    // "2026-05-18T12:34:56.789Z [ERROR] source: message"
    // "2026-05-18 12:34:56 ERROR source - message"
    // "[ERROR] message"
    let re = regex::Regex::new(
        r"^(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}[^\s]*)\s+\[?(\w+)\]?\s*[-:]?\s*(.*)"
    ).unwrap();

    if let Some(caps) = re.captures(line) {
        let level_raw = caps.get(2).map(|m| m.as_str()).unwrap_or("").to_uppercase();
        let level = if is_known_level(&level_raw) { level_raw } else { String::new() };
        let rest = caps.get(3).map(|m| m.as_str()).unwrap_or("").to_string();

        // Try to extract source from the rest
        let (source, message) = if let Some(idx) = rest.find(": ") {
            let s = &rest[..idx];
            if s.len() < 40 && !s.contains(' ') {
                (s.to_string(), rest[idx + 2..].to_string())
            } else {
                (String::new(), rest)
            }
        } else {
            (String::new(), rest)
        };

        ParsedLine {
            timestamp: caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
            level,
            source,
            message,
            raw: line.to_string(),
        }
    } else {
        // Try [LEVEL] prefix only
        let bracket_re = regex::Regex::new(r"^\[(\w+)\]\s*(.*)").unwrap();
        if let Some(caps) = bracket_re.captures(line) {
            let level_raw = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_uppercase();
            let level = if is_known_level(&level_raw) { level_raw } else { String::new() };
            ParsedLine {
                timestamp: String::new(),
                level,
                source: String::new(),
                message: caps.get(2).map(|m| m.as_str()).unwrap_or(line).to_string(),
                raw: line.to_string(),
            }
        } else {
            parse_plain_line(line)
        }
    }
}

fn parse_plain_line(line: &str) -> ParsedLine {
    let level = detect_level_from_text(line);
    ParsedLine {
        timestamp: String::new(),
        level,
        source: String::new(),
        message: line.to_string(),
        raw: line.to_string(),
    }
}

/// Detect log level from keywords in text.
fn detect_level_from_text(text: &str) -> String {
    let upper = text.to_uppercase();
    if upper.contains("FATAL") || upper.contains("CRITICAL") || upper.contains("PANIC") {
        "CRITICAL".into()
    } else if upper.contains("ERROR") || upper.contains("FAIL") || upper.contains("EXCEPTION") {
        "ERROR".into()
    } else if upper.contains("WARN") {
        "WARNING".into()
    } else if upper.contains("DEBUG") || upper.contains("TRACE") {
        "DEBUG".into()
    } else if upper.contains("INFO") {
        "INFO".into()
    } else {
        String::new()
    }
}

fn is_known_level(s: &str) -> bool {
    matches!(s, "INFO" | "WARN" | "WARNING" | "ERROR" | "DEBUG" | "TRACE" | "FATAL" | "CRITICAL" | "PANIC" | "NOTICE" | "VERBOSE")
}

/// Normalize an error message into a pattern by replacing variable parts
/// (numbers, UUIDs, IPs, paths, timestamps) with placeholders.
fn normalize_pattern(message: &str) -> String {
    let mut pattern = message.to_string();

    // Replace UUIDs
    let uuid_re = regex::Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap();
    pattern = uuid_re.replace_all(&pattern, "<UUID>").to_string();

    // Replace IP addresses
    let ip_re = regex::Regex::new(r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}").unwrap();
    pattern = ip_re.replace_all(&pattern, "<IP>").to_string();

    // Replace hex sequences (8+ chars)
    let hex_re = regex::Regex::new(r"0x[0-9a-fA-F]{4,}|[0-9a-fA-F]{8,}").unwrap();
    pattern = hex_re.replace_all(&pattern, "<HEX>").to_string();

    // Replace numbers (but keep single digits that might be meaningful)
    let num_re = regex::Regex::new(r"\b\d{2,}\b").unwrap();
    pattern = num_re.replace_all(&pattern, "<N>").to_string();

    // Replace Windows paths
    let path_re = regex::Regex::new(r"[A-Z]:\\[^\s,;]+").unwrap();
    pattern = path_re.replace_all(&pattern, "<PATH>").to_string();

    // Replace Unix paths
    let upath_re = regex::Regex::new(r"/[a-zA-Z0-9_./-]{3,}").unwrap();
    pattern = upath_re.replace_all(&pattern, "<PATH>").to_string();

    // Collapse whitespace
    let ws_re = regex::Regex::new(r"\s+").unwrap();
    pattern = ws_re.replace_all(&pattern, " ").trim().to_string();

    // Truncate very long patterns
    if pattern.len() > 200 {
        pattern = format!("{}…", crate::utils::safe_truncate(&pattern, 199));
    }

    pattern
}

/// Build timeline buckets grouping error/warning/total counts per time window.
fn build_timeline(parsed: &[ParsedLine], bucket_minutes: usize) -> Vec<TimelineBucket> {
    if parsed.is_empty() {
        return vec![];
    }

    // Group by truncated timestamp
    let mut buckets: HashMap<String, (usize, usize, usize)> = HashMap::new();

    for p in parsed {
        // Truncate timestamp to bucket size
        let bucket_key = if p.timestamp.len() >= 16 {
            // "2026-05-18T12:34:..." → truncate minutes to bucket
            if bucket_minutes >= 60 {
                // Hour bucket: "2026-05-18T12"
                if p.timestamp.len() >= 13 {
                    p.timestamp[..13].to_string()
                } else {
                    p.timestamp.clone()
                }
            } else {
                // Minute bucket: "2026-05-18T12:30"
                if p.timestamp.len() >= 16 {
                    let mins: usize = p.timestamp[14..16].parse().unwrap_or(0);
                    let bucket_min = (mins / bucket_minutes) * bucket_minutes;
                    format!("{}:{:02}", &p.timestamp[..13], bucket_min)
                } else {
                    p.timestamp.clone()
                }
            }
        } else if !p.timestamp.is_empty() {
            p.timestamp.clone()
        } else {
            continue; // Skip lines without timestamps for timeline
        };

        let entry = buckets.entry(bucket_key).or_insert((0, 0, 0));
        entry.2 += 1; // total
        match p.level.as_str() {
            "ERROR" | "CRITICAL" | "FATAL" => entry.0 += 1,
            "WARNING" | "WARN" => entry.1 += 1,
            _ => {}
        }
    }

    let mut timeline: Vec<TimelineBucket> = buckets
        .into_iter()
        .map(|(ts, (errors, warnings, total))| TimelineBucket {
            timestamp: ts,
            error_count: errors,
            warning_count: warnings,
            total_count: total,
        })
        .collect();

    timeline.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    timeline
}
