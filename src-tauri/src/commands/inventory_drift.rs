// ── inventory_drift.rs — Inventory drift detection (Sprint B #2) ────────
//
// Three commands cover the lifecycle:
//
//   • inventory_set_baseline(host_id, snapshot, label)
//       Save a point-in-time snapshot as "the trusted reference" for this
//       host. Overwrites any prior baseline (one per host by design).
//
//   • inventory_get_baseline(host_id)
//       Return the stored baseline, or None when none exists.
//
//   • inventory_compute_drift(host_id, current_snapshot)
//       Diff current against stored baseline. Returns a structured report:
//       per-category added / removed / changed lists.
//
// Diff strategy (per category):
//
//   • software:  match by name+version (case-insensitive). Same name,
//                different version = "changed" with from→to. Different
//                name = added or removed.
//   • ports:     match by port number. Different `process` on same port
//                = "changed".
//   • services:  match by name. Different `status` = "changed".
//   • certs:     match by subject. Different `expires` = "changed".
//   • scheduled: match by entry verbatim (cron lines are usually exact).
//
// What we DO NOT compute: ordering changes. If software is at index 5 in
// baseline and index 7 in current but name+version match, it's NOT drift.

use std::collections::HashSet;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::commands::metrics::shared_db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryBaseline {
    pub host_id: String,
    pub label: String,
    pub snapshot: Value,   // raw JSON — same shape the frontend sends
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DriftReport {
    pub host_id: String,
    pub has_baseline: bool,
    pub baseline_age_secs: i64,
    pub categories: Vec<DriftCategory>,
    /// Aggregate counts for the header pill.
    pub totals: DriftTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DriftTotals {
    pub added: i64,
    pub removed: i64,
    pub changed: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftCategory {
    pub name: String,         // 'software' | 'ports' | 'services' | 'certs' | 'scheduled'
    pub added: Vec<Value>,
    pub removed: Vec<Value>,
    pub changed: Vec<DriftChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftChange {
    pub identifier: String,  // human-readable key (name, port, subject…)
    pub field: String,       // which field drifted ('version', 'status', 'expires'…)
    pub from: String,
    pub to: String,
}

/// Save or overwrite the baseline for a host.
#[tauri::command]
pub async fn inventory_set_baseline(
    host_id: String,
    snapshot: Value,
    label: Option<String>,
) -> Result<(), String> {
    let label = label.unwrap_or_default();
    let snap_str = serde_json::to_string(&snapshot)
        .map_err(|e| format!("Snapshot is not valid JSON: {}", e))?;
    shared_db(move |conn| {
        conn.execute(
            "INSERT INTO inventory_baselines (host_id, label, snapshot_json, updated_at)
             VALUES (?1, ?2, ?3, strftime('%s','now'))
             ON CONFLICT(host_id) DO UPDATE SET
                 label = excluded.label,
                 snapshot_json = excluded.snapshot_json,
                 updated_at = excluded.updated_at",
            params![host_id, label, snap_str],
        ).map_err(|e| format!("inventory_set_baseline: {}", e))?;
        Ok(())
    })
}

#[tauri::command]
pub async fn inventory_get_baseline(host_id: String) -> Result<Option<InventoryBaseline>, String> {
    shared_db(move |conn| {
        let row = conn.query_row(
            "SELECT host_id, label, snapshot_json, created_at, updated_at
             FROM inventory_baselines WHERE host_id = ?1",
            params![host_id],
            |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            )),
        ).ok();
        match row {
            None => Ok(None),
            Some((hid, label, snap_str, created_at, updated_at)) => {
                let snapshot: Value = serde_json::from_str(&snap_str)
                    .unwrap_or(Value::Null);
                Ok(Some(InventoryBaseline {
                    host_id: hid, label, snapshot, created_at, updated_at,
                }))
            }
        }
    })
}

#[tauri::command]
pub async fn inventory_delete_baseline(host_id: String) -> Result<(), String> {
    shared_db(move |conn| {
        conn.execute(
            "DELETE FROM inventory_baselines WHERE host_id = ?1",
            params![host_id],
        ).map_err(|e| format!("inventory_delete_baseline: {}", e))?;
        Ok(())
    })
}

/// Compute the drift report for `host_id`'s current snapshot vs its baseline.
/// If no baseline is stored, returns `has_baseline=false` with empty categories.
#[tauri::command]
pub async fn inventory_compute_drift(
    host_id: String,
    current_snapshot: Value,
) -> Result<DriftReport, String> {
    // Pull baseline first; if absent, return empty report.
    let baseline = inventory_get_baseline(host_id.clone()).await?;
    let mut report = DriftReport {
        host_id: host_id.clone(),
        has_baseline: baseline.is_some(),
        baseline_age_secs: 0,
        categories: Vec::new(),
        totals: DriftTotals::default(),
    };
    let Some(baseline) = baseline else { return Ok(report); };
    let now = chrono::Local::now().timestamp();
    report.baseline_age_secs = now - baseline.updated_at;

    // Run each category's diff in turn, totalling counts as we go.
    let bump_totals = |added: usize, removed: usize, changed: usize, r: &mut DriftReport| {
        r.totals.added   += added as i64;
        r.totals.removed += removed as i64;
        r.totals.changed += changed as i64;
    };

    // ── Software (key = name lowercased) ────────────────────────────────
    let sw_cat = diff_software(&baseline.snapshot, &current_snapshot);
    bump_totals(sw_cat.added.len(), sw_cat.removed.len(), sw_cat.changed.len(), &mut report);
    report.categories.push(sw_cat);

    // ── Ports (key = port number) ───────────────────────────────────────
    let p_cat = diff_ports(&baseline.snapshot, &current_snapshot);
    bump_totals(p_cat.added.len(), p_cat.removed.len(), p_cat.changed.len(), &mut report);
    report.categories.push(p_cat);

    // ── Services (key = service name) ───────────────────────────────────
    let s_cat = diff_services(&baseline.snapshot, &current_snapshot);
    bump_totals(s_cat.added.len(), s_cat.removed.len(), s_cat.changed.len(), &mut report);
    report.categories.push(s_cat);

    // ── Certs (key = subject) ───────────────────────────────────────────
    let c_cat = diff_certs(&baseline.snapshot, &current_snapshot);
    bump_totals(c_cat.added.len(), c_cat.removed.len(), c_cat.changed.len(), &mut report);
    report.categories.push(c_cat);

    // ── Scheduled (key = entry verbatim) ────────────────────────────────
    let sc_cat = diff_scheduled(&baseline.snapshot, &current_snapshot);
    bump_totals(sc_cat.added.len(), sc_cat.removed.len(), sc_cat.changed.len(), &mut report);
    report.categories.push(sc_cat);

    Ok(report)
}

// ── Per-category diff helpers ────────────────────────────────────────────

fn as_array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(|x| x.as_array()).map(|a| a.as_slice()).unwrap_or(&[])
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn diff_software(baseline: &Value, current: &Value) -> DriftCategory {
    let base = as_array(baseline, "software");
    let cur  = as_array(current,  "software");

    // Map base by lowercase name → entry. We only consider the FIRST entry
    // per name to keep semantics simple; in practice software lists rarely
    // have duplicate names.
    let mut base_by_name: std::collections::HashMap<String, &Value> =
        std::collections::HashMap::new();
    for s in base {
        let n = str_field(s, "name").to_lowercase();
        if !n.is_empty() { base_by_name.entry(n).or_insert(s); }
    }
    let mut cur_by_name: std::collections::HashMap<String, &Value> =
        std::collections::HashMap::new();
    for s in cur {
        let n = str_field(s, "name").to_lowercase();
        if !n.is_empty() { cur_by_name.entry(n).or_insert(s); }
    }
    let mut added: Vec<Value> = Vec::new();
    let mut removed: Vec<Value> = Vec::new();
    let mut changed: Vec<DriftChange> = Vec::new();
    let base_names: HashSet<&String> = base_by_name.keys().collect();
    let cur_names:  HashSet<&String> = cur_by_name.keys().collect();
    for n in cur_names.difference(&base_names) {
        if let Some(v) = cur_by_name.get(*n) { added.push((*v).clone()); }
    }
    for n in base_names.difference(&cur_names) {
        if let Some(v) = base_by_name.get(*n) { removed.push((*v).clone()); }
    }
    for n in base_names.intersection(&cur_names) {
        let b = base_by_name[*n];
        let c = cur_by_name[*n];
        let bv = str_field(b, "version");
        let cv = str_field(c, "version");
        if bv != cv {
            changed.push(DriftChange {
                identifier: str_field(c, "name"),
                field: "version".into(),
                from: bv, to: cv,
            });
        }
    }
    DriftCategory { name: "software".into(), added, removed, changed }
}

fn diff_ports(baseline: &Value, current: &Value) -> DriftCategory {
    let base = as_array(baseline, "ports");
    let cur  = as_array(current,  "ports");
    fn port_num(v: &Value) -> i64 {
        v.get("port").and_then(|x| x.as_i64()).unwrap_or(-1)
    }
    let mut base_by_port: std::collections::HashMap<i64, &Value> = std::collections::HashMap::new();
    for p in base { let k = port_num(p); if k >= 0 { base_by_port.entry(k).or_insert(p); } }
    let mut cur_by_port:  std::collections::HashMap<i64, &Value> = std::collections::HashMap::new();
    for p in cur  { let k = port_num(p); if k >= 0 { cur_by_port.entry(k).or_insert(p); } }
    let base_keys: HashSet<i64> = base_by_port.keys().copied().collect();
    let cur_keys:  HashSet<i64> = cur_by_port.keys().copied().collect();
    let mut added: Vec<Value> = cur_keys.difference(&base_keys)
        .filter_map(|k| cur_by_port.get(k).map(|v| (*v).clone())).collect();
    let mut removed: Vec<Value> = base_keys.difference(&cur_keys)
        .filter_map(|k| base_by_port.get(k).map(|v| (*v).clone())).collect();
    // Stable order for the UI
    added.sort_by_key(|v| v.get("port").and_then(|x| x.as_i64()).unwrap_or(0));
    removed.sort_by_key(|v| v.get("port").and_then(|x| x.as_i64()).unwrap_or(0));

    let mut changed: Vec<DriftChange> = Vec::new();
    for k in base_keys.intersection(&cur_keys) {
        let b = base_by_port[k];
        let c = cur_by_port[k];
        let bp = str_field(b, "process");
        let cp = str_field(c, "process");
        if bp != cp {
            changed.push(DriftChange {
                identifier: format!("port {}", k),
                field: "process".into(),
                from: bp, to: cp,
            });
        }
    }
    changed.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    DriftCategory { name: "ports".into(), added, removed, changed }
}

fn diff_services(baseline: &Value, current: &Value) -> DriftCategory {
    let base = as_array(baseline, "services");
    let cur  = as_array(current,  "services");
    let mut bb: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
    for s in base { let n = str_field(s, "name"); if !n.is_empty() { bb.entry(n).or_insert(s); } }
    let mut cc: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
    for s in cur  { let n = str_field(s, "name"); if !n.is_empty() { cc.entry(n).or_insert(s); } }
    let bk: HashSet<&String> = bb.keys().collect();
    let ck: HashSet<&String> = cc.keys().collect();
    let added: Vec<Value> = ck.difference(&bk).filter_map(|n| cc.get(*n).map(|v| (*v).clone())).collect();
    let removed: Vec<Value> = bk.difference(&ck).filter_map(|n| bb.get(*n).map(|v| (*v).clone())).collect();
    let mut changed: Vec<DriftChange> = Vec::new();
    for n in bk.intersection(&ck) {
        let b = bb[*n]; let c = cc[*n];
        let bs = str_field(b, "status");
        let cs = str_field(c, "status");
        if bs != cs {
            changed.push(DriftChange {
                identifier: (*n).clone(),
                field: "status".into(),
                from: bs, to: cs,
            });
        }
    }
    DriftCategory { name: "services".into(), added, removed, changed }
}

fn diff_certs(baseline: &Value, current: &Value) -> DriftCategory {
    let base = as_array(baseline, "certs");
    let cur  = as_array(current,  "certs");
    let mut bb: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
    for v in base { let k = str_field(v, "subject"); if !k.is_empty() { bb.entry(k).or_insert(v); } }
    let mut cc: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
    for v in cur  { let k = str_field(v, "subject"); if !k.is_empty() { cc.entry(k).or_insert(v); } }
    let bk: HashSet<&String> = bb.keys().collect();
    let ck: HashSet<&String> = cc.keys().collect();
    let added: Vec<Value> = ck.difference(&bk).filter_map(|n| cc.get(*n).map(|v| (*v).clone())).collect();
    let removed: Vec<Value> = bk.difference(&ck).filter_map(|n| bb.get(*n).map(|v| (*v).clone())).collect();
    let mut changed: Vec<DriftChange> = Vec::new();
    for n in bk.intersection(&ck) {
        let b = bb[*n]; let c = cc[*n];
        let be = str_field(b, "expires");
        let ce = str_field(c, "expires");
        if be != ce {
            changed.push(DriftChange {
                identifier: (*n).clone(),
                field: "expires".into(),
                from: be, to: ce,
            });
        }
    }
    DriftCategory { name: "certs".into(), added, removed, changed }
}

fn diff_scheduled(baseline: &Value, current: &Value) -> DriftCategory {
    let base = as_array(baseline, "scheduled");
    let cur  = as_array(current,  "scheduled");
    // Match by `entry` verbatim — cron lines are usually byte-exact.
    let bset: HashSet<String> = base.iter().map(|v| str_field(v, "entry")).filter(|s| !s.is_empty()).collect();
    let cset: HashSet<String> = cur.iter().map(|v| str_field(v, "entry")).filter(|s| !s.is_empty()).collect();
    let added: Vec<Value> = cur.iter()
        .filter(|v| !bset.contains(&str_field(v, "entry")) && !str_field(v, "entry").is_empty())
        .cloned().collect();
    let removed: Vec<Value> = base.iter()
        .filter(|v| !cset.contains(&str_field(v, "entry")) && !str_field(v, "entry").is_empty())
        .cloned().collect();
    DriftCategory { name: "scheduled".into(), added, removed, changed: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snap(software: Vec<Value>, ports: Vec<Value>, services: Vec<Value>) -> Value {
        json!({
            "software": software,
            "ports": ports,
            "services": services,
            "certs": [],
            "scheduled": [],
        })
    }

    #[test]
    fn software_added_and_removed_correctly() {
        let base = snap(
            vec![json!({"name": "OpenSSL", "version": "1.0.1"}),
                 json!({"name": "Python",  "version": "3.11"})],
            vec![], vec![],
        );
        let cur = snap(
            vec![json!({"name": "OpenSSL", "version": "1.0.1"}),
                 // Python removed, NodeJS added.
                 json!({"name": "NodeJS",  "version": "20.0"})],
            vec![], vec![],
        );
        let c = diff_software(&base, &cur);
        assert_eq!(c.added.len(),   1, "NodeJS should be added");
        assert_eq!(c.removed.len(), 1, "Python should be removed");
        assert_eq!(c.changed.len(), 0, "OpenSSL same version → no change");
    }

    #[test]
    fn software_version_drift_flagged_as_changed() {
        let base = snap(vec![json!({"name": "OpenSSL", "version": "1.0.1"})], vec![], vec![]);
        let cur  = snap(vec![json!({"name": "OpenSSL", "version": "3.0.0"})], vec![], vec![]);
        let c = diff_software(&base, &cur);
        assert_eq!(c.added.len(), 0);
        assert_eq!(c.removed.len(), 0);
        assert_eq!(c.changed.len(), 1);
        assert_eq!(c.changed[0].field, "version");
        assert_eq!(c.changed[0].from, "1.0.1");
        assert_eq!(c.changed[0].to,   "3.0.0");
    }

    #[test]
    fn ports_added_and_process_change_detected() {
        let base = snap(vec![], vec![
            json!({"port": 80, "process": "nginx"}),
            json!({"port": 22, "process": "sshd"}),
        ], vec![]);
        let cur = snap(vec![], vec![
            json!({"port": 80, "process": "apache"}),  // changed
            json!({"port": 22, "process": "sshd"}),    // same
            json!({"port": 443,"process": "nginx"}),   // added
        ], vec![]);
        let c = diff_ports(&base, &cur);
        assert_eq!(c.added.len(),   1);
        assert_eq!(c.removed.len(), 0);
        assert_eq!(c.changed.len(), 1);
        assert_eq!(c.changed[0].identifier, "port 80");
        assert_eq!(c.changed[0].from, "nginx");
        assert_eq!(c.changed[0].to,   "apache");
    }

    #[test]
    fn services_status_drift_flagged() {
        let base = snap(vec![], vec![], vec![
            json!({"name": "spooler", "status": "Running"}),
        ]);
        let cur = snap(vec![], vec![], vec![
            json!({"name": "spooler", "status": "Stopped"}),
        ]);
        let c = diff_services(&base, &cur);
        assert_eq!(c.changed.len(), 1);
        assert_eq!(c.changed[0].from, "Running");
        assert_eq!(c.changed[0].to,   "Stopped");
    }

    #[test]
    fn empty_baseline_yields_all_added() {
        let base = snap(vec![], vec![], vec![]);
        let cur = snap(
            vec![json!({"name": "OpenSSL", "version": "3.0"})],
            vec![json!({"port": 22, "process": "sshd"})],
            vec![json!({"name": "spooler", "status": "Running"})],
        );
        let sw = diff_software(&base, &cur);
        let p  = diff_ports(&base, &cur);
        let sv = diff_services(&base, &cur);
        assert_eq!(sw.added.len() + p.added.len() + sv.added.len(), 3);
        assert_eq!(sw.removed.len() + p.removed.len() + sv.removed.len(), 0);
    }

    #[test]
    fn case_insensitive_software_name_match() {
        // Sometimes a vendor capitalizes differently across uninstall lists.
        // We MUST treat "OpenSSL" and "openssl" as the same package — otherwise
        // every drift report shows phantom added/removed pairs.
        let base = snap(vec![json!({"name": "OpenSSL", "version": "1.0"})], vec![], vec![]);
        let cur  = snap(vec![json!({"name": "openssl", "version": "1.0"})], vec![], vec![]);
        let c = diff_software(&base, &cur);
        assert_eq!(c.added.len(), 0, "case-only difference must NOT count as added");
        assert_eq!(c.removed.len(), 0, "case-only difference must NOT count as removed");
        assert_eq!(c.changed.len(), 0, "same version → no change either");
    }
}
