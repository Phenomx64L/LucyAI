// ── CAPACITY PLANNING — Historical metrics + trend projection (P0 Feature 3) ──
//
// Samples system metrics (CPU, RAM, disk) at regular intervals and stores them
// in SQLite. Provides downsampled hourly averages for long-term trends, and
// linear regression for "days until full" projections.
//
// The frontend calls `save_metrics_sample` every 5 minutes from its dashboard
// polling interval. `get_capacity_trends` returns the data needed for 7/30/90
// day charts plus projection calculations.

use serde::{Deserialize, Serialize};
use crate::commands::metrics::shared_db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityTrend {
    /// Raw or downsampled data points for charting
    pub samples: Vec<TrendPoint>,
    /// Linear projection: estimated days until disk reaches threshold
    pub disk_days_until_full: Option<f64>,
    /// Linear projection: estimated days until RAM exceeds threshold
    pub ram_days_until_critical: Option<f64>,
    /// Current values (latest sample)
    pub current: Option<TrendPoint>,
    /// Summary statistics
    pub stats: TrendStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendPoint {
    pub ts: i64,
    pub cpu: f64,
    pub ram: f64,
    pub disk: f64,
    pub ram_mb: i64,
    pub disk_gb: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendStats {
    pub avg_cpu: f64,
    pub avg_ram: f64,
    pub avg_disk: f64,
    pub max_cpu: f64,
    pub max_ram: f64,
    pub max_disk: f64,
    pub sample_count: i64,
    pub span_hours: f64,
}

/// Save a metrics sample. Called from frontend every 5 min when dashboard is open,
/// or from a background tick. If `host_id` is empty, reads local system metrics.
#[tauri::command]
pub async fn save_metrics_sample(
    host_id: Option<String>,
    cpu: Option<f64>,
    ram: Option<f64>,
    disk: Option<f64>,
    ram_mb: Option<i64>,
    disk_gb: Option<i64>,
    disk_total_gb: Option<i64>,
    ram_total_mb: Option<i64>,
) -> Result<i64, String> {
    // If no values provided, sample local system automatically
    let (c, r, d, rmb, dgb, dtgb, rtmb) = if cpu.is_some() {
        (
            cpu.unwrap_or(0.0),
            ram.unwrap_or(0.0),
            disk.unwrap_or(0.0),
            ram_mb.unwrap_or(0),
            disk_gb.unwrap_or(0),
            disk_total_gb.unwrap_or(0),
            ram_total_mb.unwrap_or(0),
        )
    } else {
        // Auto-sample local system
        let (c, r, d, rmb, dgb, dtgb, rtmb) = tokio::task::spawn_blocking(|| {
            sample_local_system()
        })
        .await
        .map_err(|e| format!("spawn_blocking: {}", e))?;
        (c, r, d, rmb, dgb, dtgb, rtmb)
    };

    let hid = host_id.unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    tokio::task::spawn_blocking(move || {
        shared_db(|conn| {
            conn.execute(
                "INSERT INTO metrics_samples (host_id, ts, cpu, ram, disk, ram_mb, disk_gb, disk_total_gb, ram_total_mb, is_hourly)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)",
                rusqlite::params![hid, now, c, r, d, rmb, dgb, dtgb, rtmb],
            )
            .map_err(|e| format!("Insert metrics sample: {}", e))?;
            Ok(conn.last_insert_rowid())
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Get capacity trends for a host over a given number of days.
/// Returns downsampled hourly points + linear projection.
#[tauri::command]
pub async fn get_capacity_trends(
    host_id: Option<String>,
    days: Option<i64>,
) -> Result<CapacityTrend, String> {
    let hid = host_id.unwrap_or_default();
    let d = days.unwrap_or(7).min(365).max(1);

    tokio::task::spawn_blocking(move || {
        shared_db(|conn| {
            let cutoff = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64
                - (d * 86400);

            // Fetch samples (prefer hourly for >7 days, raw for <=7)
            let use_hourly = d > 7;
            let sql = if use_hourly {
                "SELECT ts, cpu, ram, disk, ram_mb, disk_gb FROM metrics_samples
                 WHERE host_id = ?1 AND ts >= ?2 AND is_hourly = 1
                 ORDER BY ts ASC LIMIT 2160"
            } else {
                "SELECT ts, cpu, ram, disk, ram_mb, disk_gb FROM metrics_samples
                 WHERE host_id = ?1 AND ts >= ?2
                 ORDER BY ts ASC LIMIT 2016"
            };

            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let samples: Vec<TrendPoint> = stmt
                .query_map(rusqlite::params![hid, cutoff], |row| {
                    Ok(TrendPoint {
                        ts:      row.get(0)?,
                        cpu:     row.get(1)?,
                        ram:     row.get(2)?,
                        disk:    row.get(3)?,
                        ram_mb:  row.get(4)?,
                        disk_gb: row.get(5)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();

            // Latest sample as "current"
            let current = samples.last().cloned();

            // Statistics
            let count = samples.len() as i64;
            let stats = if count > 0 {
                let (mut sum_cpu, mut sum_ram, mut sum_disk) = (0.0, 0.0, 0.0);
                let (mut max_cpu, mut max_ram, mut max_disk) = (0.0_f64, 0.0_f64, 0.0_f64);
                for s in &samples {
                    sum_cpu += s.cpu;
                    sum_ram += s.ram;
                    sum_disk += s.disk;
                    max_cpu = max_cpu.max(s.cpu);
                    max_ram = max_ram.max(s.ram);
                    max_disk = max_disk.max(s.disk);
                }
                let span_secs = if count > 1 {
                    (samples.last().unwrap().ts - samples.first().unwrap().ts) as f64
                } else {
                    0.0
                };
                TrendStats {
                    avg_cpu: sum_cpu / count as f64,
                    avg_ram: sum_ram / count as f64,
                    avg_disk: sum_disk / count as f64,
                    max_cpu,
                    max_ram,
                    max_disk,
                    sample_count: count,
                    span_hours: span_secs / 3600.0,
                }
            } else {
                TrendStats {
                    avg_cpu: 0.0, avg_ram: 0.0, avg_disk: 0.0,
                    max_cpu: 0.0, max_ram: 0.0, max_disk: 0.0,
                    sample_count: 0, span_hours: 0.0,
                }
            };

            // Linear regression for "days until full" (disk and RAM)
            let disk_days = if count >= 10 {
                linear_days_until(&samples, |s| s.disk, 95.0)
            } else {
                None
            };
            let ram_days = if count >= 10 {
                linear_days_until(&samples, |s| s.ram, 95.0)
            } else {
                None
            };

            Ok(CapacityTrend {
                samples,
                disk_days_until_full: disk_days,
                ram_days_until_critical: ram_days,
                current,
                stats,
            })
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Downsample raw samples into hourly averages. Idempotent — only processes
/// rows not yet downsampled. Should run periodically (e.g. daily).
// ── Tier A #3 — Predictive overlays for Dashboard ─────────────────────────
//
// Returns the regression coefficients (slope, intercept) of each metric AND
// the projected series at N evenly-spaced future timestamps. The frontend
// draws the projected curve as a dashed line + a shaded band whose width
// scales with 1 - r² (lower fit = wider uncertainty).
//
// Why a new command instead of bolting onto get_capacity_trends: that
// command already returns up to 2160 samples; adding a parallel projection
// triples the payload for the common case (dashboard refresh). Keeping
// projection a SEPARATE call means the dashboard only pays for it when the
// overlay is actually rendered.

#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricRegression {
    /// Slope in percent per day (positive = growing)
    pub slope: f64,
    /// Y-intercept (percent at the start of the regression window)
    pub intercept: f64,
    /// Coefficient of determination, 0..1 (1 = perfect fit)
    pub r_squared: f64,
    /// Sample count used in the fit
    pub samples_used: i64,
    /// Projected (ts, value) pairs at evenly-spaced future timestamps
    pub projection: Vec<(i64, f64)>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectionOverlay {
    pub cpu:   Option<MetricRegression>,
    pub ram:   Option<MetricRegression>,
    pub disk:  Option<MetricRegression>,
    /// Wall-clock at which projections were computed — the frontend uses it
    /// to align the overlay against the last observed sample without
    /// trusting client-side time (which can be skewed).
    pub computed_at: i64,
}

/// Compute a linear regression overlay for the next `forecast_days` days.
///
/// `forecast_points` controls how dense the projected series is (capped to
/// 240 — enough for hourly resolution over 10 days). If you need finer
/// granularity, call with fewer days.
#[tauri::command]
pub async fn capacity_projection(
    host_id: Option<String>,
    days: Option<i64>,
    forecast_days: Option<i64>,
    forecast_points: Option<i64>,
) -> Result<ProjectionOverlay, String> {
    let hid = host_id.unwrap_or_default();
    let d   = days.unwrap_or(14).min(365).max(2);
    let fd  = forecast_days.unwrap_or(7).min(60).max(1);
    let fp  = forecast_points.unwrap_or(24).min(240).max(2);

    tokio::task::spawn_blocking(move || {
        shared_db(|conn| {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let cutoff = now_secs - (d * 86400);
            let use_hourly = d > 7;
            let sql = if use_hourly {
                "SELECT ts, cpu, ram, disk FROM metrics_samples
                 WHERE host_id = ?1 AND ts >= ?2 AND is_hourly = 1
                 ORDER BY ts ASC LIMIT 2160"
            } else {
                "SELECT ts, cpu, ram, disk FROM metrics_samples
                 WHERE host_id = ?1 AND ts >= ?2
                 ORDER BY ts ASC LIMIT 2016"
            };
            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let samples: Vec<(i64, f64, f64, f64)> = stmt
                .query_map(rusqlite::params![hid, cutoff], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?,
                        row.get::<_, f64>(2)?, row.get::<_, f64>(3)?))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();

            if samples.len() < 5 {
                // Not enough data for a meaningful regression.
                return Ok(ProjectionOverlay {
                    cpu: None, ram: None, disk: None,
                    computed_at: now_secs,
                });
            }

            let t0 = samples[0].0 as f64;
            let project = |extract: fn(&(i64, f64, f64, f64)) -> f64| -> Option<MetricRegression> {
                fit_linear(&samples, t0, extract, now_secs, fd, fp)
            };
            Ok(ProjectionOverlay {
                cpu:  project(|s| s.1),
                ram:  project(|s| s.2),
                disk: project(|s| s.3),
                computed_at: now_secs,
            })
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// OLS linear regression of y = a*x + b over (ts→days from t0, extracted y).
/// Returns the model parameters PLUS a projected series at evenly-spaced
/// future timestamps.
fn fit_linear(
    samples: &[(i64, f64, f64, f64)],
    t0: f64,
    extract: fn(&(i64, f64, f64, f64)) -> f64,
    now_secs: i64,
    forecast_days: i64,
    forecast_points: i64,
) -> Option<MetricRegression> {
    let n = samples.len() as f64;
    if n < 5.0 { return None; }
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_xx = 0.0;
    for s in samples {
        let x = (s.0 as f64 - t0) / 86400.0; // days from first sample
        let y = extract(s);
        sum_x += x; sum_y += y; sum_xy += x * y; sum_xx += x * x;
    }
    let denom = n * sum_xx - sum_x * sum_x;
    if denom.abs() < 1e-10 { return None; }
    let slope = (n * sum_xy - sum_x * sum_y) / denom;
    let intercept = (sum_y - slope * sum_x) / n;
    // Coefficient of determination R²
    let mean_y = sum_y / n;
    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    for s in samples {
        let x = (s.0 as f64 - t0) / 86400.0;
        let y = extract(s);
        let y_pred = intercept + slope * x;
        ss_res += (y - y_pred).powi(2);
        ss_tot += (y - mean_y).powi(2);
    }
    let r_squared = if ss_tot.abs() < 1e-10 { 1.0 } else { 1.0 - ss_res / ss_tot };

    // Project: from now_secs to now_secs + forecast_days, evenly spaced.
    let mut projection = Vec::with_capacity(forecast_points as usize);
    let step_secs = (forecast_days * 86400) / (forecast_points - 1).max(1);
    for i in 0..forecast_points {
        let ts = now_secs + i * step_secs;
        let x = (ts as f64 - t0) / 86400.0;
        let mut y = intercept + slope * x;
        // Clamp to [0, 110] — metrics are percentages; a slight overshoot is
        // useful so the line still shows "you'll cross 95%" rather than
        // capping invisibly at 100.
        y = y.max(0.0).min(110.0);
        projection.push((ts, y));
    }

    Some(MetricRegression {
        slope, intercept, r_squared,
        samples_used: samples.len() as i64,
        projection,
    })
}

#[tauri::command]
pub async fn downsample_metrics() -> Result<i64, String> {
    tokio::task::spawn_blocking(|| {
        shared_db(|conn| {
            // Find the latest hourly sample timestamp
            let latest_hourly: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(ts), 0) FROM metrics_samples WHERE is_hourly = 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            // Group raw samples older than 1 hour into hourly buckets
            let cutoff = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64
                - 3600; // at least 1 hour old

            let inserted = conn
                .execute(
                    "INSERT INTO metrics_samples (host_id, ts, cpu, ram, disk, ram_mb, disk_gb, disk_total_gb, ram_total_mb, is_hourly)
                     SELECT host_id,
                            (ts / 3600) * 3600 AS bucket,
                            AVG(cpu), AVG(ram), AVG(disk),
                            CAST(AVG(ram_mb) AS INTEGER),
                            CAST(AVG(disk_gb) AS INTEGER),
                            MAX(disk_total_gb),
                            MAX(ram_total_mb),
                            1
                     FROM metrics_samples
                     WHERE is_hourly = 0 AND ts > ?1 AND ts < ?2
                     GROUP BY host_id, bucket
                     HAVING COUNT(*) > 0",
                    rusqlite::params![latest_hourly, cutoff],
                )
                .map_err(|e| format!("Downsample insert: {}", e))?;

            // Prune raw samples older than 7 days
            let week_ago = cutoff - (6 * 86400);
            conn.execute(
                "DELETE FROM metrics_samples WHERE is_hourly = 0 AND ts < ?1",
                rusqlite::params![week_ago],
            )
            .map_err(|e| format!("Prune raw samples: {}", e))?;

            // Prune hourly samples older than 90 days
            let ninety_days_ago = cutoff - (89 * 86400);
            conn.execute(
                "DELETE FROM metrics_samples WHERE is_hourly = 1 AND ts < ?1",
                rusqlite::params![ninety_days_ago],
            )
            .map_err(|e| format!("Prune hourly samples: {}", e))?;

            Ok(inserted as i64)
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Sample local system metrics using sysinfo.
fn sample_local_system() -> (f64, f64, f64, i64, i64, i64, i64) {
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu = sys.global_cpu_info().cpu_usage() as f64;
    let total_mem = sys.total_memory() / 1_048_576; // MB
    let used_mem = sys.used_memory() / 1_048_576;
    let ram_pct = if total_mem > 0 {
        (used_mem as f64 / total_mem as f64) * 100.0
    } else {
        0.0
    };

    // Primary disk (usually C:\)
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let (disk_pct, disk_used_gb, disk_total_gb) = disks
        .iter()
        .next()
        .map(|d| {
            let total = d.total_space() / 1_073_741_824;
            let free = d.available_space() / 1_073_741_824;
            let used = total.saturating_sub(free);
            let pct = if total > 0 {
                (used as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            (pct, used as i64, total as i64)
        })
        .unwrap_or((0.0, 0, 0));

    (
        (cpu * 10.0).round() / 10.0,
        (ram_pct * 10.0).round() / 10.0,
        (disk_pct * 10.0).round() / 10.0,
        used_mem as i64,
        disk_used_gb,
        disk_total_gb,
        total_mem as i64,
    )
}

/// Simple linear regression: given samples and an extractor function,
/// project how many days until the value reaches `threshold` (percent).
/// Returns None if slope is ≤ 0 (not growing) or projection > 365 days.
fn linear_days_until<F>(samples: &[TrendPoint], extract: F, threshold: f64) -> Option<f64>
where
    F: Fn(&TrendPoint) -> f64,
{
    let n = samples.len() as f64;
    if n < 2.0 {
        return None;
    }

    // Use timestamps as x-values (in days from first sample)
    let t0 = samples[0].ts as f64;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_xx = 0.0;

    for s in samples {
        let x = (s.ts as f64 - t0) / 86400.0; // days
        let y = extract(s);
        sum_x += x;
        sum_y += y;
        sum_xy += x * y;
        sum_xx += x * x;
    }

    let denom = n * sum_xx - sum_x * sum_x;
    if denom.abs() < 1e-10 {
        return None;
    }

    let slope = (n * sum_xy - sum_x * sum_y) / denom;
    let intercept = (sum_y - slope * sum_x) / n;

    if slope <= 0.01 {
        return None; // Not growing meaningfully
    }

    // Current x (days from t0)
    let current_x = (samples.last().unwrap().ts as f64 - t0) / 86400.0;
    let current_y = intercept + slope * current_x;

    if current_y >= threshold {
        return Some(0.0); // Already at threshold
    }

    let days_to_threshold = (threshold - current_y) / slope;
    if days_to_threshold > 365.0 || days_to_threshold < 0.0 {
        None
    } else {
        Some((days_to_threshold * 10.0).round() / 10.0)
    }
}
