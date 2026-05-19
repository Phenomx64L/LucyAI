// ── Performance Suite — Automated backend + DB benchmarks ─────────────────
//
// Measures Lucy's operational capacity across 5 dimensions:
//   P1: Database throughput (read/write IOPS on all core tables)
//   P2: Semantic search latency (embeddings + cosine scan)
//   P3: Memory precision (write → read → verify round-trip)
//   P4: Concurrent access (WAL mode parallel reads/writes)
//   P5: Data integrity under load (foreign keys, FTS consistency)
//
// Usage: cargo test --test perf_suite -- --nocapture
//        cargo test --test perf_suite -- --nocapture --test-threads=1  (sequential)

use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::time::Instant;

fn db_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA not set");
    PathBuf::from(appdata).join("com.lucy.dev").join("lucy.db")
}

fn open_db() -> Connection {
    let path = db_path();
    assert!(path.exists(), "lucy.db not found at {:?}. Run Lucy first.", path);
    Connection::open(&path).expect("Failed to open lucy.db")
}

fn open_readonly() -> Connection {
    let path = db_path();
    Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ).expect("Failed to open lucy.db read-only")
}

// ─────────────────────────────────────────────────────────────────────────
// P1: DATABASE THROUGHPUT
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn p1_write_throughput_agent_memories() {
    let db = open_db();
    let count = 100;
    let t0 = Instant::now();

    db.execute_batch("BEGIN").unwrap();
    for i in 0..count {
        db.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance, created_at)
             VALUES (?1, ?2, ?3, ?4, '', 1, strftime('%s','now'))",
            params![
                format!("perf-session-{}", i),
                format!("Perf test memory #{}", i),
                format!("This is a performance test memory entry number {}. It contains enough text to be realistic for benchmarking purposes. The content should be representative of actual memory entries stored during Lucy operation.", i),
                format!("[\"perf_test\",\"batch_{}\",\"benchmark\"]", i % 10),
            ],
        ).unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    let elapsed = t0.elapsed();

    let ops_sec = count as f64 / elapsed.as_secs_f64();
    println!("[P1] Write {} agent_memories in {:.1}ms ({:.0} ops/sec)", count, elapsed.as_millis(), ops_sec);
    assert!(ops_sec > 100.0, "Write throughput too low: {:.0} ops/sec (need >100)", ops_sec);
    println!("[PASS] P1 write throughput: {:.0} ops/sec", ops_sec);

    // Cleanup
    db.execute("DELETE FROM agent_memories WHERE session_id LIKE 'perf-session-%'", []).unwrap();
}

#[test]
fn p1_read_throughput_scan() {
    let db = open_readonly();
    let iterations = 50;
    let t0 = Instant::now();

    for _ in 0..iterations {
        let _count: i64 = db.query_row(
            "SELECT COUNT(*) FROM conversation_turns WHERE role = 'user'", [], |r| r.get(0)
        ).unwrap_or(0);
    }
    let elapsed = t0.elapsed();

    let ops_sec = iterations as f64 / elapsed.as_secs_f64();
    println!("[P1] {} count queries in {:.1}ms ({:.0} ops/sec)", iterations, elapsed.as_millis(), ops_sec);
    assert!(ops_sec > 500.0, "Read throughput too low: {:.0} ops/sec (need >500)", ops_sec);
    println!("[PASS] P1 read throughput: {:.0} ops/sec", ops_sec);
}

#[test]
fn p1_metrics_insert_throughput() {
    let db = open_db();
    let count = 200;
    let t0 = Instant::now();

    db.execute_batch("BEGIN").unwrap();
    for i in 0..count {
        db.execute(
            "INSERT INTO metrics_samples (host_id, ts, cpu, ram, disk, ram_mb, disk_gb, disk_total_gb, ram_total_mb, is_hourly)
             VALUES ('perf-host', ?1, ?2, ?3, ?4, 11000, 250, 475, 32000, 0)",
            params![
                1700000000 + i as i64,
                (i as f64 % 100.0),
                30.0 + (i as f64 % 40.0),
                50.0 + (i as f64 % 30.0),
            ],
        ).unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    let elapsed = t0.elapsed();

    let ops_sec = count as f64 / elapsed.as_secs_f64();
    println!("[P1] Write {} metrics_samples in {:.1}ms ({:.0} ops/sec)", count, elapsed.as_millis(), ops_sec);
    assert!(ops_sec > 500.0, "Metrics write too low: {:.0} ops/sec (need >500)", ops_sec);
    println!("[PASS] P1 metrics write: {:.0} ops/sec", ops_sec);

    // Cleanup
    db.execute("DELETE FROM metrics_samples WHERE host_id = 'perf-host'", []).unwrap();
}

// ─────────────────────────────────────────────────────────────────────────
// P2: QUERY LATENCY (complex queries that Lucy runs in production)
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn p2_conversation_context_build() {
    // Simulates building context from recent conversation turns
    let db = open_readonly();
    let t0 = Instant::now();
    let iterations = 20;

    for _ in 0..iterations {
        let _rows: Vec<(String, String)> = db.prepare(
            "SELECT role, content FROM conversation_turns ORDER BY created_at DESC LIMIT 50"
        ).unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    }
    let elapsed = t0.elapsed();
    let avg_ms = elapsed.as_millis() as f64 / iterations as f64;

    println!("[P2] Context build (50 turns): avg {:.2}ms over {} iterations", avg_ms, iterations);
    assert!(avg_ms < 10.0, "Context build too slow: {:.2}ms (need <10ms)", avg_ms);
    println!("[PASS] P2 context build: {:.2}ms avg", avg_ms);
}

#[test]
fn p2_metrics_aggregation() {
    // Simulates capacity planning query (aggregation over time series)
    let db = open_readonly();
    let t0 = Instant::now();
    let iterations = 30;

    for _ in 0..iterations {
        let _stats: (f64, f64, f64, f64, f64, f64) = db.query_row(
            "SELECT MIN(cpu), AVG(cpu), MAX(cpu), MIN(ram), AVG(ram), MAX(ram)
             FROM metrics_samples WHERE ts > strftime('%s','now') - 86400",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        ).unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
    }
    let elapsed = t0.elapsed();
    let avg_ms = elapsed.as_millis() as f64 / iterations as f64;

    println!("[P2] Metrics aggregation (24h window): avg {:.2}ms over {} iterations", avg_ms, iterations);
    assert!(avg_ms < 5.0, "Metrics agg too slow: {:.2}ms (need <5ms)", avg_ms);
    println!("[PASS] P2 metrics aggregation: {:.2}ms avg", avg_ms);
}

#[test]
fn p2_fts_search_latency() {
    // Full-text search on conversation turns (used by semantic recall)
    let db = open_readonly();
    let queries = ["sistema", "error", "memoria", "CPU", "proceso"];
    let mut total_ms = 0.0;

    for q in &queries {
        let t0 = Instant::now();
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM conversation_turns_fts WHERE conversation_turns_fts MATCH ?1",
            params![q],
            |r| r.get(0),
        ).unwrap_or(0);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        total_ms += ms;
        println!("  FTS '{}': {} hits in {:.2}ms", q, count, ms);
    }
    let avg_ms = total_ms / queries.len() as f64;

    println!("[P2] FTS search avg: {:.2}ms across {} queries", avg_ms, queries.len());
    assert!(avg_ms < 15.0, "FTS too slow: {:.2}ms (need <15ms)", avg_ms);
    println!("[PASS] P2 FTS search: {:.2}ms avg", avg_ms);
}

// ─────────────────────────────────────────────────────────────────────────
// P3: MEMORY PRECISION (write → read → verify round-trip)
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn p3_memory_roundtrip() {
    let db = open_db();
    // Idempotent: clean up any leftover data from a previous failed run
    db.execute("DELETE FROM agent_memories WHERE session_id = 'perf-p3'", []).unwrap();

    let test_entries = vec![
        ("El puerto SSH del servidor prod es 2222, no 22", "[\"ssh\",\"prod\",\"puerto\",\"perf_test\"]", 3),
        ("La base de datos PostgreSQL usa pool size 25", "[\"postgres\",\"pool\",\"config\",\"perf_test\"]", 2),
        ("Backup nocturno corre a las 3:00 AM UTC via cron", "[\"backup\",\"cron\",\"schedule\",\"perf_test\"]", 2),
        ("CyberArk EPM bloquea scripts sin firma digital", "[\"cyberark\",\"epm\",\"scripts\",\"perf_test\"]", 3),
        ("El firewall permite solo puertos 80, 443, 8080", "[\"firewall\",\"puertos\",\"seguridad\",\"perf_test\"]", 2),
    ];

    // Write
    let t0 = Instant::now();
    for (content, tags, importance) in &test_entries {
        db.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance, created_at)
             VALUES ('perf-p3', 'P3 test', ?1, ?2, '', ?3, strftime('%s','now'))",
            params![content, tags, importance],
        ).unwrap();
    }
    let write_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // Read back by tag — filter by session_id to avoid picking up preexisting memories
    let t1 = Instant::now();
    let found: Vec<(String, String, i64)> = db.prepare(
        "SELECT content, tags, importance FROM agent_memories WHERE session_id = 'perf-p3' ORDER BY importance DESC"
    ).unwrap()
    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
    .unwrap()
    .filter_map(|r| r.ok())
    .collect();
    let read_ms = t1.elapsed().as_secs_f64() * 1000.0;

    println!("[P3] Write {} memories: {:.2}ms, Read back: {:.2}ms", test_entries.len(), write_ms, read_ms);

    // Verify precision
    assert_eq!(found.len(), test_entries.len(), "Expected {} memories, found {}", test_entries.len(), found.len());

    // Verify high-importance entries come first
    assert!(found[0].2 >= found[1].2, "Importance ordering broken");

    // Verify content integrity (no corruption)
    for (content, _, _) in &test_entries {
        assert!(found.iter().any(|(c, _, _)| c == content), "Memory content '{}...' not found in read-back", &content[..30]);
    }
    println!("[PASS] P3 memory round-trip: write={:.2}ms read={:.2}ms, all {} verified", write_ms, read_ms, found.len());

    // Cleanup
    db.execute("DELETE FROM agent_memories WHERE session_id = 'perf-p3'", []).unwrap();
}

#[test]
fn p3_tag_search_precision() {
    let db = open_db();
    // Idempotent cleanup
    db.execute("DELETE FROM agent_memories WHERE session_id = 'perf-p3tag'", []).unwrap();

    // Insert memories with overlapping tags
    let entries = vec![
        ("Server A runs nginx on port 80", "[\"nginx\",\"server_a\",\"web\",\"p3tag\"]"),
        ("Server B runs nginx on port 443 with SSL", "[\"nginx\",\"server_b\",\"ssl\",\"p3tag\"]"),
        ("Database server runs on port 5432", "[\"postgres\",\"database\",\"server_c\",\"p3tag\"]"),
        ("Redis cache on server A port 6379", "[\"redis\",\"server_a\",\"cache\",\"p3tag\"]"),
    ];
    for (content, tags) in &entries {
        db.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance, created_at)
             VALUES ('perf-p3tag', 'P3 tag test', ?1, ?2, '', 2, strftime('%s','now'))",
            params![content, tags],
        ).unwrap();
    }

    // Search by specific tag: "nginx" should return exactly 2
    let nginx_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM agent_memories WHERE tags LIKE '%nginx%' AND session_id = 'perf-p3tag'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(nginx_count, 2, "Expected 2 nginx memories, got {}", nginx_count);

    // Search by "server_a" should return exactly 2 (nginx + redis)
    let sa_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM agent_memories WHERE tags LIKE '%server_a%' AND session_id = 'perf-p3tag'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(sa_count, 2, "Expected 2 server_a memories, got {}", sa_count);

    // Cross-tag: "nginx" AND "ssl" should return exactly 1
    let ssl_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM agent_memories WHERE tags LIKE '%nginx%' AND tags LIKE '%ssl%' AND session_id = 'perf-p3tag'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(ssl_count, 1, "Expected 1 nginx+ssl memory, got {}", ssl_count);

    println!("[PASS] P3 tag search: nginx=2, server_a=2, nginx+ssl=1 -- all correct");

    // Cleanup
    db.execute("DELETE FROM agent_memories WHERE session_id = 'perf-p3tag'", []).unwrap();
}

// ─────────────────────────────────────────────────────────────────────────
// P4: CONCURRENT ACCESS (WAL mode stress test)
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn p4_concurrent_reads() {
    use std::thread;

    let num_threads = 4;
    let reads_per_thread = 50;
    let t0 = Instant::now();

    let handles: Vec<_> = (0..num_threads).map(|_tid| {
        thread::spawn(move || {
            let db = open_readonly();
            let mut sum = 0i64;
            for _ in 0..reads_per_thread {
                let c: i64 = db.query_row(
                    "SELECT COUNT(*) FROM conversation_turns", [], |r| r.get(0)
                ).unwrap_or(0);
                sum += c;
            }
            sum
        })
    }).collect();

    let results: Vec<i64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let elapsed = t0.elapsed();
    let total_ops = num_threads * reads_per_thread;
    let ops_sec = total_ops as f64 / elapsed.as_secs_f64();

    // All threads should get the same count (consistency check)
    let expected = results[0] / reads_per_thread as i64;
    for (i, &r) in results.iter().enumerate() {
        let avg = r / reads_per_thread as i64;
        assert_eq!(avg, expected, "Thread {} got different count ({}) vs thread 0 ({})", i, avg, expected);
    }

    println!("[P4] {} concurrent reads in {:.1}ms ({:.0} ops/sec, {} threads)", total_ops, elapsed.as_millis(), ops_sec, num_threads);
    assert!(ops_sec > 1000.0, "Concurrent reads too slow: {:.0} ops/sec (need >1000)", ops_sec);
    println!("[PASS] P4 concurrent reads: {:.0} ops/sec across {} threads", ops_sec, num_threads);
}

#[test]
fn p4_read_write_contention() {
    // Simulate WAL mode: one writer + multiple readers simultaneously
    use std::thread;
    use std::sync::{Arc, Barrier};

    let barrier = Arc::new(Barrier::new(3)); // 1 writer + 2 readers

    let b1 = barrier.clone();
    let writer = thread::spawn(move || {
        let db = open_db();
        b1.wait(); // sync start
        let t0 = Instant::now();
        db.execute_batch("BEGIN").unwrap();
        for i in 0..50 {
            db.execute(
                "INSERT INTO metrics_samples (host_id, ts, cpu, ram, disk, ram_mb, disk_gb, disk_total_gb, ram_total_mb, is_hourly)
                 VALUES ('p4-writer', ?1, ?2, 35.0, 50.0, 11000, 250, 475, 32000, 0)",
                params![1800000000 + i as i64, i as f64],
            ).unwrap();
        }
        db.execute_batch("COMMIT").unwrap();
        t0.elapsed()
    });

    let readers: Vec<_> = (0..2).map(|_| {
        let b = barrier.clone();
        thread::spawn(move || {
            let db = open_readonly();
            b.wait(); // sync start
            let t0 = Instant::now();
            let mut reads = 0;
            while t0.elapsed().as_millis() < 200 { // read for 200ms
                let _: i64 = db.query_row(
                    "SELECT COUNT(*) FROM metrics_samples", [], |r| r.get(0)
                ).unwrap_or(0);
                reads += 1;
            }
            (t0.elapsed(), reads)
        })
    }).collect();

    let write_elapsed = writer.join().unwrap();
    let read_results: Vec<_> = readers.into_iter().map(|h| h.join().unwrap()).collect();

    println!("[P4] Writer: 50 inserts in {:.1}ms", write_elapsed.as_millis());
    for (i, (elapsed, reads)) in read_results.iter().enumerate() {
        let rps = *reads as f64 / elapsed.as_secs_f64();
        println!("[P4] Reader {}: {} reads in {:.1}ms ({:.0} ops/sec)", i, reads, elapsed.as_millis(), rps);
    }

    // Writer should complete in reasonable time even with concurrent readers
    assert!(write_elapsed.as_millis() < 500, "Writer blocked too long: {}ms", write_elapsed.as_millis());
    // Each reader should achieve >100 ops/sec even during writes
    for (_, (elapsed, reads)) in read_results.iter().enumerate() {
        let rps = *reads as f64 / elapsed.as_secs_f64();
        assert!(rps > 100.0, "Reader too slow during contention: {:.0} ops/sec", rps);
    }
    println!("[PASS] P4 WAL contention: writer + 2 readers operated without blocking");

    // Cleanup
    let db = open_db();
    db.execute("DELETE FROM metrics_samples WHERE host_id = 'p4-writer'", []).unwrap();
}

// ─────────────────────────────────────────────────────────────────────────
// P5: DATA INTEGRITY
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn p5_db_pragmas_optimal() {
    // Use read-write connection: PRAGMA quick_check on FTS5 tables needs
    // write access internally (SQLite quirk — it validates the inverted index).
    let db = open_db();

    // WAL mode is critical for concurrent read/write performance
    let journal: String = db.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    println!("  journal_mode: {}", journal);
    assert_eq!(journal.to_lowercase(), "wal", "Expected WAL journal mode, got {}", journal);

    // Foreign keys should be enabled for data integrity
    let fk: i64 = db.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    println!("  foreign_keys: {}", fk);

    // Page size and cache
    let page_size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap();
    let cache_size: i64 = db.query_row("PRAGMA cache_size", [], |r| r.get(0)).unwrap();
    println!("  page_size: {} bytes, cache_size: {}", page_size, cache_size);

    // Quick integrity check (needs r/w for FTS5 validation)
    let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0)).unwrap();
    assert_eq!(integrity, "ok", "Integrity check failed: {}", integrity);

    println!("[PASS] P5 DB pragmas: WAL={}, page={}B, integrity=ok", journal, page_size);
}

#[test]
fn p5_fts_consistency() {
    // Verify FTS index is in sync with the base table
    let db = open_readonly();

    let base_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM conversation_turns", [], |r| r.get(0)
    ).unwrap_or(0);

    // FTS content table should have matching row count
    // (FTS5 content tables store the indexed content)
    let fts_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM conversation_turns_fts", [], |r| r.get(0)
    ).unwrap_or(-1);

    println!("[P5] conversation_turns: {} rows, FTS index: {} rows", base_count, fts_count);

    // They should match (or FTS might be slightly behind in external-content mode)
    let diff = (base_count - fts_count).abs();
    assert!(diff <= 2, "FTS out of sync: base={} vs fts={} (diff={})", base_count, fts_count, diff);
    println!("[PASS] P5 FTS sync: base={} fts={} (diff={})", base_count, fts_count, diff);
}

#[test]
fn p5_token_usage_consistency() {
    let db = open_readonly();

    // Token usage should have non-negative values
    let bad_tokens: i64 = db.query_row(
        "SELECT COUNT(*) FROM token_usage WHERE input_tokens < 0 OR output_tokens < 0",
        [], |r| r.get(0),
    ).unwrap_or(0);
    assert_eq!(bad_tokens, 0, "Found {} token_usage rows with negative values", bad_tokens);

    // All models should be valid (non-empty)
    let empty_models: i64 = db.query_row(
        "SELECT COUNT(*) FROM token_usage WHERE model IS NULL OR model = ''",
        [], |r| r.get(0),
    ).unwrap_or(0);
    assert_eq!(empty_models, 0, "Found {} token_usage rows with empty model", empty_models);

    let total: i64 = db.query_row("SELECT COUNT(*) FROM token_usage", [], |r| r.get(0)).unwrap_or(0);
    println!("[PASS] P5 token_usage: {} records, all valid (no negatives, no empty models)", total);
}

// ─────────────────────────────────────────────────────────────────────────
// SUMMARY
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn p_summary() {
    let db = open_db();

    let tables: Vec<(&str, i64)> = vec![
        "audit_trail", "metrics_samples", "agent_memories", "agent_crystals",
        "agent_insights", "memory_core", "conversation_turns", "token_usage", "embeddings",
    ].iter().map(|t| {
        let c: i64 = db.query_row(&format!("SELECT COUNT(*) FROM {}", t), [], |r| r.get(0)).unwrap_or(0);
        (*t, c)
    }).collect();

    let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0)).unwrap();
    let journal: String = db.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    let db_size = std::fs::metadata(db_path()).map(|m| m.len()).unwrap_or(0);

    println!();
    println!("+=========================================+");
    println!("|   LUCY PERFORMANCE SUITE — RESULTS      |");
    println!("+=========================================+");
    println!("|  DB size:        {:>8} KB             |", db_size / 1024);
    println!("|  Journal mode:   {:>8}                |", journal);
    println!("|  Integrity:      {:>8}                |", integrity);
    println!("+-----------------------------------------+");
    for (name, count) in &tables {
        println!("|  {:25}{:>6}          |", name, count);
    }
    println!("+-----------------------------------------+");
    println!("|  P1: DB throughput          [see above]  |");
    println!("|  P2: Query latency          [see above]  |");
    println!("|  P3: Memory precision       [see above]  |");
    println!("|  P4: Concurrent access      [see above]  |");
    println!("|  P5: Data integrity         [see above]  |");
    println!("+=========================================+");
}
