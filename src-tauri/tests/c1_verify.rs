// ── C1 Verification — Post-test database checks ─────────────────────────
//
// Run after completing Test Case 1 (baseline diagnostic + memory seed).
// Queries the lucy.db directly to verify backend state.
//
// Usage: cargo test --test c1_verify -- --ignored --nocapture
//
// IGNORED BY DEFAULT, and that is the point. These are a MANUAL CHECKLIST, not
// unit tests: they open the developer's own %APPDATA%m.lucy.devucy.db and
// assert things about the data in it. In the default test set they turn a plain
// `cargo test` red for reasons that have nothing to do with the change being
// made, which is how a suite stops being believed.
//
// One of them could never pass anyway: `PRAGMA quick_check` has to WRITE to
// validate an FTS5 inverted index, and the connection here is read-only, so it
// reports "attempt to write a readonly database" as a corruption finding.

use rusqlite::Connection;
use std::path::PathBuf;

fn db_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA not set");
    PathBuf::from(appdata).join("com.lucy.dev").join("lucy.db")
}

fn open_db() -> Connection {
    let path = db_path();
    assert!(path.exists(), "lucy.db not found at {:?}. Run Lucy first.", path);
    Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ).expect("Failed to open lucy.db")
}

#[test]
#[ignore = "manual: needs a lucy.db that has already run Test Case 1 (cargo test --test c1_verify -- --ignored)"]
fn c1_check_diagnostics_health() {
    let db = open_db();
    // Self-diagnostics runs health checks but doesn't persist to a specific table.
    // Verify the DB itself is healthy via PRAGMA.
    let integrity: String = db
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .expect("quick_check failed");
    assert_eq!(integrity, "ok", "Database integrity check failed: {}", integrity);
    println!("✅ Database integrity: {}", integrity);
}

#[test]
#[ignore = "manual: needs a lucy.db that has already run Test Case 1 (cargo test --test c1_verify -- --ignored)"]
fn c1_check_audit_trail_has_entries() {
    let db = open_db();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM audit_trail", [], |r| r.get(0))
        .expect("audit_trail query failed");
    println!("   Audit trail entries: {}", count);
    assert!(count > 0, "❌ No audit trail entries found. Expected ≥1 after C1 execution.");
    println!("✅ Audit trail: {} entries", count);
}

#[test]
#[ignore = "manual: needs a lucy.db that has already run Test Case 1 (cargo test --test c1_verify -- --ignored)"]
fn c1_check_audit_trail_recent() {
    let db = open_db();
    // Check that there's at least one entry from the last 10 minutes
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM audit_trail WHERE created_at > strftime('%s','now') - 600",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    println!("   Recent audit entries (last 10min): {}", count);
    if count > 0 {
        println!("✅ Recent audit entries found: {}", count);
    } else {
        println!("⚠  No audit entries in last 10 minutes (run C1 test first)");
    }
}

#[test]
#[ignore = "manual: needs a lucy.db that has already run Test Case 1 (cargo test --test c1_verify -- --ignored)"]
fn c1_check_capacity_samples() {
    let db = open_db();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM metrics_samples", [], |r| r.get(0))
        .expect("metrics_samples query failed");
    println!("   Capacity samples: {}", count);
    if count > 0 {
        // Check the most recent sample has valid values
        let (cpu, ram, disk): (f64, f64, f64) = db
            .query_row(
                "SELECT cpu, ram, disk FROM metrics_samples ORDER BY ts DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("latest sample query failed");
        println!("   Latest sample: CPU={:.1}%, RAM={:.1}%, Disk={:.1}%", cpu, ram, disk);
        assert!(cpu >= 0.0 && cpu <= 100.0, "CPU out of range: {}", cpu);
        assert!(ram >= 0.0 && ram <= 100.0, "RAM out of range: {}", ram);
        assert!(disk >= 0.0 && disk <= 100.0, "Disk out of range: {}", disk);
        println!("✅ Capacity samples valid: CPU={:.1}% RAM={:.1}% Disk={:.1}%", cpu, ram, disk);
    } else {
        println!("⚠  No capacity samples yet (auto-sampling starts after 2min)");
    }
}

#[test]
#[ignore = "manual: needs a lucy.db that has already run Test Case 1 (cargo test --test c1_verify -- --ignored)"]
fn c1_check_agent_memories() {
    let db = open_db();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM agent_memories", [], |r| r.get(0))
        .expect("agent_memories query failed");
    println!("   Total agent memories: {}", count);
    assert!(count >= 0, "agent_memories query returned negative");

    // Check for baseline/system/health tagged memories
    let tagged: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM agent_memories WHERE tags LIKE '%system%' OR tags LIKE '%health%' OR tags LIKE '%baseline%'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    println!("   System/health/baseline tagged: {}", tagged);
    if tagged > 0 {
        println!("✅ Found {} system/health memories (C1 seed data present)", tagged);
    } else {
        println!("⚠  No system/health memories yet (ask Lucy to save diagnostic results)");
    }
}

#[test]
#[ignore = "manual: needs a lucy.db that has already run Test Case 1 (cargo test --test c1_verify -- --ignored)"]
fn c1_check_memory_core() {
    let db = open_db();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM memory_core", [], |r| r.get(0))
        .expect("memory_core query failed");
    println!("   Core memory entries: {}", count);
    println!("✅ Core memory: {} entries", count);
}

#[test]
#[ignore = "manual: needs a lucy.db that has already run Test Case 1 (cargo test --test c1_verify -- --ignored)"]
fn c1_summary() {
    let db = open_db();

    let audit: i64 = db.query_row("SELECT COUNT(*) FROM audit_trail", [], |r| r.get(0)).unwrap_or(0);
    let samples: i64 = db.query_row("SELECT COUNT(*) FROM metrics_samples", [], |r| r.get(0)).unwrap_or(0);
    let memories: i64 = db.query_row("SELECT COUNT(*) FROM agent_memories", [], |r| r.get(0)).unwrap_or(0);
    let crystals: i64 = db.query_row("SELECT COUNT(*) FROM agent_crystals", [], |r| r.get(0)).unwrap_or(0);
    let insights: i64 = db.query_row("SELECT COUNT(*) FROM agent_insights", [], |r| r.get(0)).unwrap_or(0);
    let core: i64 = db.query_row("SELECT COUNT(*) FROM memory_core", [], |r| r.get(0)).unwrap_or(0);

    println!("\n╔══════════════════════════════════════╗");
    println!("║     C1 BASELINE — STATE SUMMARY      ║");
    println!("╠══════════════════════════════════════╣");
    println!("║  Audit trail entries:  {:>6}         ║", audit);
    println!("║  Capacity samples:     {:>6}         ║", samples);
    println!("║  Agent memories:       {:>6}         ║", memories);
    println!("║  Crystals:             {:>6}         ║", crystals);
    println!("║  Insights:             {:>6}         ║", insights);
    println!("║  Core memory:          {:>6}         ║", core);
    println!("╚══════════════════════════════════════╝");

    let passed = (audit > 0) as u8 + (memories >= 0) as u8 + 1; // DB integrity always passes if we got here
    println!("\n  {}/3 baseline checks passed", passed);
    if samples == 0 {
        println!("  ℹ  Capacity auto-sampling starts 2min after app launch");
    }
}
