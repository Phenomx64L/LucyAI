// ── F6 — Cross-App Object Bridge ──────────────────────────────────────────
//
// Keeps structured data alive across LLM turns so Lucy can re-query
// previously-fetched objects WITHOUT re-running the original command.
//
// Why this matters:
//   PowerShell returns rich PSCustomObject arrays — services, processes,
//   net connections. Today they get flattened to text the moment Lucy
//   sends them through the LLM. Then "of those, the ones older than 30
//   days" requires re-running the whole query.
//
//   With this bridge: the OBJECT lives in-memory keyed by session. The
//   LLM can call <TOOL>obj_query:expr</TOOL> against the stored items —
//   filtering by field, sorting, projecting — without another shell call.
//
// Storage:
//   - In-process HashMap<session_id, Vec<StoredObject>>
//   - Each entry has TTL 30min from last-access — GC sweeps every 5min
//   - Bounded: max 100 objects per session, max 5000 chars per object
//     (anything bigger gets stored as truncated + flag)
//
// Query language (deliberately small, deterministic):
//   - select <field1>,<field2>            project columns
//   - where <field> <op> <value>          filter rows  (ops: =, !=, >, <, >=, <=, contains)
//   - orderby <field> [desc]              sort
//   - limit <N>                           take N
//   - count                               return count only
//
// Composability via pipes:
//   "where Name contains chrome | orderby WorkingSet desc | limit 5"
//
// This is intentionally NOT a full DSL. The LLM (or the user) can chain
// the operators it knows — anything beyond should fall back to a fresh
// PowerShell call.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use once_cell::sync::Lazy;

const MAX_OBJECTS_PER_SESSION: usize = 100;
const MAX_PAYLOAD_CHARS: usize = 5000;
const TTL_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredObject {
    pub key: String,
    pub source: String,            // free-form ("ps:Get-Service", "tool:netconn", etc.)
    pub stored_at: i64,
    pub last_accessed: i64,
    pub row_count: usize,
    pub fields: Vec<String>,
    /// Rows as JSON values (always an array of records or scalars).
    pub rows: serde_json::Value,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreArgs {
    pub session_id: String,
    pub key: String,                  // logical name used in queries ("services", "procs", "last")
    pub source: String,
    /// JSON payload — either array of records (Vec<Map<String, Value>>) or
    /// already serialized JSON string. The function tries to be lenient.
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryArgs {
    pub session_id: String,
    /// Either an object key followed by " | ..." or just an expression
    /// that starts with "last" to use most-recent object.
    pub expression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub key: String,
    pub source: String,
    pub returned_rows: usize,
    pub original_rows: usize,
    pub rows: serde_json::Value,
    pub is_count_only: bool,
    pub count: Option<i64>,
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64).unwrap_or(0)
}

type Sessions = HashMap<String, Vec<StoredObject>>;

static STORE: Lazy<Mutex<Sessions>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Sweep expired objects across all sessions. Called from store/query paths.
fn evict_expired(store: &mut Sessions) {
    let cutoff = now_ts() - TTL_SECS;
    for objs in store.values_mut() {
        objs.retain(|o| o.last_accessed >= cutoff);
    }
    // Remove empty sessions
    store.retain(|_, objs| !objs.is_empty());
}

#[tauri::command]
pub async fn obj_bridge_store(args: StoreArgs) -> Result<StoredObject, String> {
    // Parse payload: accept either JSON array, JSON object {data: [...]},
    // or fail with a clear error.
    let parsed: serde_json::Value = serde_json::from_str(&args.payload_json)
        .map_err(|e| format!("payload_json is not valid JSON: {}", e))?;
    let rows = match &parsed {
        serde_json::Value::Array(_) => parsed.clone(),
        serde_json::Value::Object(map) => {
            if let Some(v) = map.get("data").or_else(|| map.get("rows")).or_else(|| map.get("items")) {
                v.clone()
            } else {
                // Single record → wrap as array
                serde_json::Value::Array(vec![parsed.clone()])
            }
        }
        _ => return Err("payload must be an array or object".into()),
    };

    let arr = rows.as_array().ok_or("rows is not an array")?.clone();
    let row_count = arr.len();
    // Field discovery from first record
    let fields: Vec<String> = arr.iter().next()
        .and_then(|v| v.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();

    // Truncate payload if oversized
    let serialized = serde_json::to_string(&rows).unwrap_or_default();
    let truncated = serialized.len() > MAX_PAYLOAD_CHARS;
    let final_rows = if truncated {
        // Keep first N records that fit
        let mut acc: Vec<serde_json::Value> = Vec::new();
        let mut total = 2; // brackets
        for r in &arr {
            let s = serde_json::to_string(r).unwrap_or_default();
            if total + s.len() + 1 > MAX_PAYLOAD_CHARS { break; }
            total += s.len() + 1;
            acc.push(r.clone());
        }
        serde_json::Value::Array(acc)
    } else {
        rows
    };

    let obj = StoredObject {
        key: args.key.clone(),
        source: args.source.clone(),
        stored_at: now_ts(),
        last_accessed: now_ts(),
        row_count,
        fields,
        rows: final_rows,
        truncated,
    };

    let mut store = STORE.lock().map_err(|e| format!("lock: {}", e))?;
    evict_expired(&mut store);
    let entries = store.entry(args.session_id.clone()).or_default();
    // Replace existing entry with same key
    entries.retain(|o| o.key != args.key);
    entries.push(obj.clone());
    // Cap per-session
    if entries.len() > MAX_OBJECTS_PER_SESSION {
        let excess = entries.len() - MAX_OBJECTS_PER_SESSION;
        entries.drain(0..excess);
    }
    Ok(obj)
}

#[tauri::command]
pub async fn obj_bridge_list(session_id: String) -> Result<Vec<StoredObject>, String> {
    let mut store = STORE.lock().map_err(|e| format!("lock: {}", e))?;
    evict_expired(&mut store);
    Ok(store.get(&session_id).cloned().unwrap_or_default())
}

#[tauri::command]
pub async fn obj_bridge_clear(session_id: String, key: Option<String>) -> Result<usize, String> {
    let mut store = STORE.lock().map_err(|e| format!("lock: {}", e))?;
    let removed = match key {
        Some(k) => {
            if let Some(entries) = store.get_mut(&session_id) {
                let before = entries.len();
                entries.retain(|o| o.key != k);
                before - entries.len()
            } else { 0 }
        }
        None => {
            let removed = store.get(&session_id).map(|v| v.len()).unwrap_or(0);
            store.remove(&session_id);
            removed
        }
    };
    Ok(removed)
}

// ── Query parser ─────────────────────────────────────────────────────────

#[derive(Debug)]
enum Stage {
    Select(Vec<String>),
    Where { field: String, op: String, value: String },
    OrderBy { field: String, desc: bool },
    Limit(usize),
    Count,
}

fn parse_pipeline(expr: &str) -> Result<Vec<Stage>, String> {
    let mut stages = Vec::new();
    for raw in expr.split('|') {
        let part = raw.trim();
        if part.is_empty() { continue; }
        let lower = part.to_lowercase();
        if lower == "count" {
            stages.push(Stage::Count);
        } else if lower.starts_with("select ") {
            let cols: Vec<String> = part[7..].split(',').map(|s| s.trim().to_string()).collect();
            stages.push(Stage::Select(cols));
        } else if lower.starts_with("where ") {
            // where <field> <op> <value...>
            let rest = part[6..].trim();
            // Op tokens — order matters (longest first)
            let ops = [">=", "<=", "!=", "=", ">", "<", "contains"];
            let mut found = None;
            for op in ops.iter() {
                if let Some(idx) = rest.find(op) {
                    let field = rest[..idx].trim().to_string();
                    let value = rest[idx + op.len()..].trim().trim_matches('"').trim_matches('\'').to_string();
                    if !field.is_empty() {
                        found = Some(Stage::Where { field, op: op.to_string(), value });
                        break;
                    }
                }
            }
            stages.push(found.ok_or_else(|| format!("where: cannot parse '{}'", rest))?);
        } else if lower.starts_with("orderby ") {
            let rest = part[8..].trim();
            let (field, desc) = if let Some(stripped) = rest.strip_suffix(" desc") {
                (stripped.trim().to_string(), true)
            } else if let Some(stripped) = rest.strip_suffix(" asc") {
                (stripped.trim().to_string(), false)
            } else {
                (rest.to_string(), false)
            };
            stages.push(Stage::OrderBy { field, desc });
        } else if lower.starts_with("limit ") {
            let n: usize = part[6..].trim().parse()
                .map_err(|_| format!("limit: not a number: '{}'", &part[6..]))?;
            stages.push(Stage::Limit(n));
        } else {
            return Err(format!("unknown stage: '{}'", part));
        }
    }
    Ok(stages)
}

fn val_to_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b)   => b.to_string(),
        _ => v.to_string(),
    }
}

fn val_to_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn compare(left: &serde_json::Value, op: &str, right: &str) -> bool {
    match op {
        "=" | "!=" => {
            let eq = val_to_str(left).eq_ignore_ascii_case(right);
            if op == "=" { eq } else { !eq }
        }
        "contains" => val_to_str(left).to_lowercase().contains(&right.to_lowercase()),
        ">" | "<" | ">=" | "<=" => {
            let l = val_to_f64(left);
            let r: Option<f64> = right.parse().ok();
            match (l, r) {
                (Some(a), Some(b)) => match op {
                    ">"  => a >  b,
                    "<"  => a <  b,
                    ">=" => a >= b,
                    "<=" => a <= b,
                    _    => false,
                },
                _ => false,
            }
        }
        _ => false,
    }
}

#[tauri::command]
pub async fn obj_bridge_query(args: QueryArgs) -> Result<QueryResult, String> {
    let mut store = STORE.lock().map_err(|e| format!("lock: {}", e))?;
    evict_expired(&mut store);

    // Resolve target object: first token before the first '|' OR "last" alias
    let (target_key, pipeline_expr) = if let Some(idx) = args.expression.find('|') {
        (args.expression[..idx].trim().to_string(), args.expression[idx + 1..].to_string())
    } else {
        (args.expression.trim().to_string(), String::new())
    };

    let entries = store.get_mut(&args.session_id).ok_or("no objects stored for this session")?;
    let obj_ref: &mut StoredObject = if target_key.eq_ignore_ascii_case("last") {
        entries.iter_mut().max_by_key(|o| o.last_accessed).ok_or("session has no objects")?
    } else {
        entries.iter_mut().find(|o| o.key == target_key)
            .ok_or_else(|| format!("no object with key '{}'", target_key))?
    };
    obj_ref.last_accessed = now_ts();

    let rows = obj_ref.rows.as_array().cloned().unwrap_or_default();
    let original_rows = rows.len();
    let stages = parse_pipeline(&pipeline_expr)?;

    let mut current = rows;
    let mut projected: Option<Vec<String>> = None;
    let mut count_only = false;

    for stage in stages {
        match stage {
            Stage::Where { field, op, value } => {
                current.retain(|row| {
                    let v = row.get(&field).unwrap_or(&serde_json::Value::Null);
                    compare(v, &op, &value)
                });
            }
            Stage::OrderBy { field, desc } => {
                current.sort_by(|a, b| {
                    let av = a.get(&field);
                    let bv = b.get(&field);
                    let (af, bf) = (av.and_then(val_to_f64), bv.and_then(val_to_f64));
                    let cmp = match (af, bf) {
                        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                        _ => {
                            let as_ = av.map(val_to_str).unwrap_or_default();
                            let bs_ = bv.map(val_to_str).unwrap_or_default();
                            as_.cmp(&bs_)
                        }
                    };
                    if desc { cmp.reverse() } else { cmp }
                });
            }
            Stage::Limit(n) => {
                current.truncate(n);
            }
            Stage::Select(cols) => {
                projected = Some(cols);
            }
            Stage::Count => {
                count_only = true;
            }
        }
    }

    // Apply projection at end
    if let Some(cols) = projected {
        current = current.into_iter().map(|row| {
            let mut m = serde_json::Map::new();
            if let Some(obj) = row.as_object() {
                for c in &cols {
                    if let Some(v) = obj.get(c) { m.insert(c.clone(), v.clone()); }
                }
            }
            serde_json::Value::Object(m)
        }).collect();
    }

    let count = if count_only { Some(current.len() as i64) } else { None };
    let returned = current.len();
    Ok(QueryResult {
        key: obj_ref.key.clone(),
        source: obj_ref.source.clone(),
        returned_rows: returned,
        original_rows,
        rows: if count_only { serde_json::Value::Array(vec![]) } else { serde_json::Value::Array(current) },
        is_count_only: count_only,
        count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_row(name: &str, ws: i64) -> serde_json::Value {
        serde_json::json!({ "Name": name, "WorkingSet": ws })
    }

    #[test]
    fn parser_handles_select_where_orderby_limit() {
        let stages = parse_pipeline("select Name,WorkingSet | where WorkingSet > 100 | orderby WorkingSet desc | limit 3").unwrap();
        assert_eq!(stages.len(), 4);
    }

    #[test]
    fn parser_rejects_unknown_stage() {
        assert!(parse_pipeline("unknown foo").is_err());
    }

    #[tokio::test]
    async fn store_and_query_pipeline() {
        let session = "test-session-1".to_string();
        let payload = serde_json::json!([
            mk_row("chrome", 500),
            mk_row("firefox", 300),
            mk_row("notepad", 50),
        ]).to_string();

        obj_bridge_store(StoreArgs {
            session_id: session.clone(),
            key: "procs".into(),
            source: "test".into(),
            payload_json: payload,
        }).await.unwrap();

        let r = obj_bridge_query(QueryArgs {
            session_id: session.clone(),
            expression: "procs | where WorkingSet > 100 | orderby WorkingSet desc".into(),
        }).await.unwrap();

        assert_eq!(r.returned_rows, 2);
        assert_eq!(r.original_rows, 3);
        let first = r.rows.as_array().unwrap()[0].get("Name").unwrap().as_str().unwrap();
        assert_eq!(first, "chrome");

        // Cleanup
        obj_bridge_clear(session, None).await.unwrap();
    }

    #[tokio::test]
    async fn contains_filter_works() {
        let session = "test-session-2".to_string();
        let payload = serde_json::json!([
            { "Name": "Adobe Reader", "Status": "Running" },
            { "Name": "Discord", "Status": "Stopped" },
            { "Name": "Adobe Acrobat", "Status": "Running" },
        ]).to_string();

        obj_bridge_store(StoreArgs {
            session_id: session.clone(),
            key: "svcs".into(),
            source: "test".into(),
            payload_json: payload,
        }).await.unwrap();

        let r = obj_bridge_query(QueryArgs {
            session_id: session.clone(),
            expression: "svcs | where Name contains adobe".into(),
        }).await.unwrap();
        assert_eq!(r.returned_rows, 2);
        obj_bridge_clear(session, None).await.unwrap();
    }
}
