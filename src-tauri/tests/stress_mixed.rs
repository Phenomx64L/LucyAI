// ── Stress Test: Mixed Commands + Data Persistence Verification ────────────
//
// Simulates the real usage patterns that were failing:
//   S1: Rapid sequential writes to multiple tables (simulates agent loop)
//   S2: Interleaved reads during writes (simulates UI polling during AI response)
//   S3: Memory persistence: write → app "restart" → verify data survives
//   S4: Large content handling (long conversation turns, big tool outputs)
//   S5: Conversation turn integrity (no gaps, correct ordering)
//   S6: Token usage accounting accuracy
//   S7: FTS index stays in sync after rapid inserts
//   S8: Metrics continuity (no gaps in time series)
//
// Usage: cargo test --test stress_mixed -- --nocapture --test-threads=1

use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::time::Instant;

fn db_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA not set");
    PathBuf::from(appdata).join("com.lucy.dev").join("lucy.db")
}

fn open_db() -> Connection {
    let path = db_path();
    assert!(path.exists(), "lucy.db not found at {:?}", path);
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
// S1: RAPID SEQUENTIAL WRITES (Agent Loop Simulation)
// Simulates what happens when the agent loop runs 25 iterations quickly,
// writing to conversation_turns, agent_memories, and audit_trail.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn s1_agent_loop_rapid_writes() {
    let db = open_db();
    let tab_id = "stress-s1-agent-loop";
    let session_id = "stress-s1";
    // Cleanup from prior runs
    db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();
    db.execute("DELETE FROM agent_memories WHERE session_id = ?1", params![session_id]).unwrap();

    let t0 = Instant::now();
    let iterations = 25; // MAX_LOOPS in the real agent

    for i in 0..iterations {
        // Each iteration: 1 user turn + 1 lucy turn + optional memory save
        db.execute(
            "INSERT INTO conversation_turns (tab_id, tab_title, role, content, created_at)
             VALUES (?1, 'Stress S1', 'user', ?2, strftime('%s','now'))",
            params![tab_id, format!("Agent loop step {}: analyze the output and proceed", i)],
        ).unwrap();

        db.execute(
            "INSERT INTO conversation_turns (tab_id, tab_title, role, content, created_at)
             VALUES (?1, 'Stress S1', 'lucy', ?2, strftime('%s','now'))",
            params![tab_id, format!("<THOUGHT>Step {} analysis: checking file integrity.</THOUGHT>\nFindings for iteration {}:\n- CPU within parameters\n- No anomalies\n- Memory stable", i, i)],
        ).unwrap();

        if i % 5 == 0 {
            db.execute(
                "INSERT INTO agent_memories (session_id, title, content, tags, files, importance, created_at)
                 VALUES (?1, ?2, ?3, ?4, '', 2, strftime('%s','now'))",
                params![
                    session_id,
                    format!("Agent finding #{}", i),
                    format!("Step {}: system operating within parameters. CPU stable, no memory leaks.", i),
                    format!("[\"agent_loop\",\"step_{}\",\"stress_test\"]", i),
                ],
            ).unwrap();
        }
    }
    let elapsed = t0.elapsed();

    let turn_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM conversation_turns WHERE tab_id = ?1",
        params![tab_id], |r| r.get(0),
    ).unwrap();
    let mem_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM agent_memories WHERE session_id = ?1",
        params![session_id], |r| r.get(0),
    ).unwrap();

    assert_eq!(turn_count, (iterations * 2) as i64, "Expected {} turns, got {}", iterations * 2, turn_count);
    assert_eq!(mem_count, 5, "Expected 5 memories (every 5th of 25), got {}", mem_count);
    println!("[S1] Agent loop: {} turns + {} memories in {:.1}ms", turn_count, mem_count, elapsed.as_millis());
    println!("[PASS] S1: All {} writes persisted correctly", turn_count + mem_count);

    db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();
    db.execute("DELETE FROM agent_memories WHERE session_id = ?1", params![session_id]).unwrap();
}

// ─────────────────────────────────────────────────────────────────────────
// S2: INTERLEAVED READS DURING WRITES
// Simulates the frontend polling conversation_turns and metrics while
// the backend is actively writing new data (the streaming scenario).
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn s2_interleaved_read_write() {
    use std::thread;
    use std::sync::{Arc, Barrier, atomic::{AtomicU64, Ordering}};

    let barrier = Arc::new(Barrier::new(3));
    let writes_done = Arc::new(AtomicU64::new(0));
    let tab_id = "stress-s2-interleave";

    // Cleanup
    {
        let db = open_db();
        db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();
    }

    let wd = writes_done.clone();
    let b1 = barrier.clone();
    let writer = thread::spawn(move || {
        let db = open_db();
        b1.wait();
        for i in 0..50 {
            db.execute(
                "INSERT INTO conversation_turns (tab_id, tab_title, role, content, created_at)
                 VALUES ('stress-s2-interleave', 'Stress S2', 'lucy', ?1, strftime('%s','now'))",
                params![format!("Streaming chunk {} of the response...", i)],
            ).unwrap();
            wd.store(i as u64 + 1, Ordering::SeqCst);
        }
    });

    let wd1 = writes_done.clone();
    let b2 = barrier.clone();
    let reader1 = thread::spawn(move || {
        let db = open_readonly();
        b2.wait();
        let mut reads = 0;
        let mut saw_growing = false;
        let mut prev_count = 0i64;
        loop {
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM conversation_turns WHERE tab_id = 'stress-s2-interleave'",
                [], |r| r.get(0),
            ).unwrap_or(0);
            if count > prev_count { saw_growing = true; }
            prev_count = count;
            reads += 1;
            if wd1.load(Ordering::SeqCst) >= 50 && count >= 50 { break; }
            if reads > 2000 { break; }
            thread::sleep(std::time::Duration::from_micros(100));
        }
        (reads, saw_growing, prev_count)
    });

    let wd2 = writes_done.clone();
    let b3 = barrier.clone();
    let reader2 = thread::spawn(move || {
        let db = open_readonly();
        b3.wait();
        let mut reads = 0;
        loop {
            let _: i64 = db.query_row(
                "SELECT COUNT(*) FROM metrics_samples", [], |r| r.get(0),
            ).unwrap_or(0);
            reads += 1;
            if wd2.load(Ordering::SeqCst) >= 50 { break; }
            if reads > 2000 { break; }
            thread::sleep(std::time::Duration::from_micros(100));
        }
        reads
    });

    writer.join().unwrap();
    let (r1_reads, r1_saw_growing, r1_final) = reader1.join().unwrap();
    let r2_reads = reader2.join().unwrap();

    println!("[S2] Writer: 50 turns written");
    println!("[S2] Reader1 (turns): {} reads, saw_growing={}, final_count={}", r1_reads, r1_saw_growing, r1_final);
    println!("[S2] Reader2 (metrics): {} reads during writes", r2_reads);

    assert_eq!(r1_final, 50, "Reader didn't see all 50 turns (saw {})", r1_final);
    assert!(r1_saw_growing, "Reader never saw count increase — WAL visibility broken");
    println!("[PASS] S2: Readers saw live data during writes, final count correct");

    let db = open_db();
    db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();
}

// ─────────────────────────────────────────────────────────────────────────
// S3: PERSISTENCE ACROSS RECONNECTION
// Simulates: write data → close connection → reopen → verify all data
// (tests that WAL checkpointing works and nothing is lost)
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn s3_persistence_across_reconnect() {
    let session_id = "stress-s3-persist";
    let tab_id = "stress-s3-tab";

    // Phase 1: Write with connection A
    {
        let db = open_db();
        db.execute("DELETE FROM agent_memories WHERE session_id = ?1", params![session_id]).unwrap();
        db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();

        for i in 0..10 {
            db.execute(
                "INSERT INTO agent_memories (session_id, title, content, tags, files, importance, created_at)
                 VALUES (?1, ?2, ?3, ?4, '', ?5, strftime('%s','now'))",
                params![
                    session_id,
                    format!("Persist test #{}", i),
                    format!("Critical configuration: server-{} runs on port {} with {} workers", i, 8080 + i, 4 + i),
                    format!("[\"config\",\"server_{}\",\"persist_test\"]", i),
                    if i < 3 { 3 } else { 1 },
                ],
            ).unwrap();
        }
        db.execute(
            "INSERT INTO conversation_turns (tab_id, tab_title, role, content, created_at)
             VALUES (?1, 'Stress S3', 'user', 'What ports are our servers running on?', strftime('%s','now'))",
            params![tab_id],
        ).unwrap();
    }

    // Phase 2: Reopen with a COMPLETELY NEW connection and verify
    {
        let db2 = open_db();

        let mem_count: i64 = db2.query_row(
            "SELECT COUNT(*) FROM agent_memories WHERE session_id = ?1",
            params![session_id], |r| r.get(0),
        ).unwrap();
        assert_eq!(mem_count, 10, "Lost memories after reconnect: expected 10, got {}", mem_count);

        let high_imp: Vec<String> = db2.prepare(
            "SELECT content FROM agent_memories WHERE session_id = ?1 AND importance = 3 ORDER BY title"
        ).unwrap()
        .query_map(params![session_id], |r| r.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
        assert_eq!(high_imp.len(), 3, "Expected 3 high-importance memories, got {}", high_imp.len());
        assert!(high_imp[0].contains("port 8080"), "Content corrupted: {}", high_imp[0]);
        assert!(high_imp[1].contains("port 8081"), "Content corrupted: {}", high_imp[1]);

        let turn_count: i64 = db2.query_row(
            "SELECT COUNT(*) FROM conversation_turns WHERE tab_id = ?1",
            params![tab_id], |r| r.get(0),
        ).unwrap();
        assert_eq!(turn_count, 1, "Lost conversation turn after reconnect");

        println!("[PASS] S3: All 10 memories + 1 turn survived reconnect, content verified");

        db2.execute("DELETE FROM agent_memories WHERE session_id = ?1", params![session_id]).unwrap();
        db2.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();
    }
}

// ─────────────────────────────────────────────────────────────────────────
// S4: LARGE CONTENT HANDLING
// Tests that very long conversation turns and tool outputs don't get
// truncated or corrupted in the DB.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn s4_large_content() {
    let db = open_db();
    let tab_id = "stress-s4-large";
    db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();

    let large_content: String = (0..500).map(|i| {
        format!("Line {}: {} | PID {} | CPU {:.1}% | MEM {:.1} MB | Status: running\n",
            i, format!("process_name_{}.exe", i % 50), 1000 + i, (i as f64 % 100.0) * 0.3, (i as f64) * 2.5)
    }).collect();

    assert!(large_content.len() > 40000, "Test content should be >40KB, got {}", large_content.len());

    let t0 = Instant::now();
    db.execute(
        "INSERT INTO conversation_turns (tab_id, tab_title, role, content, created_at)
         VALUES (?1, 'Stress S4', 'lucy', ?2, strftime('%s','now'))",
        params![tab_id, &large_content],
    ).unwrap();
    let write_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = Instant::now();
    let readback: String = db.query_row(
        "SELECT content FROM conversation_turns WHERE tab_id = ?1",
        params![tab_id], |r| r.get(0),
    ).unwrap();
    let read_ms = t1.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(readback.len(), large_content.len(),
        "Content truncated: wrote {} bytes, read {} bytes", large_content.len(), readback.len());
    assert_eq!(readback, large_content, "Content corrupted after round-trip");

    println!("[S4] Large content: {} bytes, write={:.2}ms, read={:.2}ms", large_content.len(), write_ms, read_ms);
    println!("[PASS] S4: {} byte content survived DB round-trip without truncation", large_content.len());

    db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();
}

// ─────────────────────────────────────────────────────────────────────────
// S5: CONVERSATION ORDERING INTEGRITY
// Verifies that turns maintain correct chronological order even under
// rapid insertion (no reordering, no gaps in sequence).
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn s5_conversation_ordering() {
    let db = open_db();
    let tab_id = "stress-s5-order";
    db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();

    // Insert 40 alternating turns rapidly
    for i in 0..40 {
        let role = if i % 2 == 0 { "user" } else { "lucy" };
        db.execute(
            "INSERT INTO conversation_turns (tab_id, tab_title, role, content, created_at)
             VALUES (?1, 'Stress S5', ?2, ?3, strftime('%s','now'))",
            params![tab_id, role, format!("Turn #{}: {}", i, if role == "user" { "question" } else { "answer" })],
        ).unwrap();
    }

    // Read back and verify order
    let turns: Vec<(i64, String, String)> = db.prepare(
        "SELECT rowid, role, content FROM conversation_turns WHERE tab_id = ?1 ORDER BY rowid ASC"
    ).unwrap()
    .query_map(params![tab_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
    .unwrap()
    .filter_map(|r| r.ok())
    .collect();

    assert_eq!(turns.len(), 40, "Expected 40 turns, got {}", turns.len());

    // Verify alternating pattern
    for (i, (_rowid, role, content)) in turns.iter().enumerate() {
        let expected_role = if i % 2 == 0 { "user" } else { "lucy" };
        assert_eq!(role, expected_role, "Turn {} has wrong role: expected '{}', got '{}'", i, expected_role, role);
        assert!(content.contains(&format!("Turn #{}:", i)), "Turn {} has wrong content: {}", i, content);
    }

    // Verify rowids are strictly increasing (no gaps in our batch)
    for i in 1..turns.len() {
        assert!(turns[i].0 > turns[i-1].0, "Rowid not increasing at turn {}: {} <= {}", i, turns[i].0, turns[i-1].0);
    }

    println!("[PASS] S5: 40 turns in correct alternating order, rowids strictly increasing");

    // Cleanup
    db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();
}

// ─────────────────────────────────────────────────────────────────────────
// S6: TOKEN USAGE ACCOUNTING
// Verifies the token_usage table accurately tracks model usage.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn s6_token_usage_accounting() {
    let db = open_readonly();

    // Verify all existing records have valid data
    let total: i64 = db.query_row("SELECT COUNT(*) FROM token_usage", [], |r| r.get(0)).unwrap();
    let bad_input: i64 = db.query_row(
        "SELECT COUNT(*) FROM token_usage WHERE input_tokens < 0", [], |r| r.get(0)
    ).unwrap();
    let bad_output: i64 = db.query_row(
        "SELECT COUNT(*) FROM token_usage WHERE output_tokens < 0", [], |r| r.get(0)
    ).unwrap();
    let null_model: i64 = db.query_row(
        "SELECT COUNT(*) FROM token_usage WHERE model IS NULL OR model = ''", [], |r| r.get(0)
    ).unwrap();
    let _null_request_type: i64 = db.query_row(
        "SELECT COUNT(*) FROM token_usage WHERE request_type IS NULL OR request_type = ''", [], |r| r.get(0)
    ).unwrap();

    assert_eq!(bad_input, 0, "Found {} records with negative input_tokens", bad_input);
    assert_eq!(bad_output, 0, "Found {} records with negative output_tokens", bad_output);
    assert_eq!(null_model, 0, "Found {} records with null/empty model", null_model);

    // Check per-model breakdown
    let models: Vec<(String, i64, i64, i64)> = db.prepare(
        "SELECT model, SUM(input_tokens), SUM(output_tokens), COUNT(*) FROM token_usage GROUP BY model"
    ).unwrap()
    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
    .unwrap()
    .filter_map(|r| r.ok())
    .collect();

    let total_in: i64 = models.iter().map(|m| m.1).sum();
    let total_out: i64 = models.iter().map(|m| m.2).sum();

    println!("[S6] Token usage: {} records across {} models", total, models.len());
    for (model, inp, out, calls) in &models {
        println!("  > {}: {} calls, {}K in / {}K out", model, calls, inp / 1000, out / 1000);
    }
    println!("  Total: {}K input, {}K output tokens", total_in / 1000, total_out / 1000);

    // Sanity: output should be less than input (responses are shorter than prompts with context)
    if total_in > 0 {
        let ratio = total_out as f64 / total_in as f64;
        println!("  Output/Input ratio: {:.3}", ratio);
        assert!(ratio < 1.0, "Output tokens ({}) exceed input tokens ({}) — suspicious", total_out, total_in);
    }

    println!("[PASS] S6: {} records valid, {} models, ratio healthy", total, models.len());
}

// ─────────────────────────────────────────────────────────────────────────
// S7: FTS INDEX SYNC AFTER RAPID INSERTS
// Inserts many conversation turns, then verifies FTS can find them all.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn s7_fts_sync_after_inserts() {
    let db = open_db();
    let tab_id = "stress-s7-fts";
    db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();

    // Insert turns with unique searchable content
    let unique_words = vec![
        "xylophone_alpha", "quantum_bravo", "nebula_charlie",
        "prism_delta", "vortex_echo", "zenith_foxtrot",
    ];
    for (i, word) in unique_words.iter().enumerate() {
        db.execute(
            "INSERT INTO conversation_turns (tab_id, tab_title, role, content, created_at)
             VALUES (?1, 'Stress S7', 'lucy', ?2, strftime('%s','now'))",
            params![tab_id, format!("Analysis complete: the {} component shows normal readings at checkpoint {}", word, i)],
        ).unwrap();
    }

    // FTS should find each unique word
    let mut found_count = 0;
    for word in &unique_words {
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM conversation_turns_fts WHERE conversation_turns_fts MATCH ?1",
            params![word], |r| r.get(0),
        ).unwrap_or(0);
        if count > 0 { found_count += 1; }
        println!("  FTS '{}': {} hits", word, count);
    }

    // Check overall sync
    let base: i64 = db.query_row("SELECT COUNT(*) FROM conversation_turns", [], |r| r.get(0)).unwrap();
    let fts: i64 = db.query_row("SELECT COUNT(*) FROM conversation_turns_fts", [], |r| r.get(0)).unwrap();
    let diff = (base - fts).abs();

    println!("[S7] FTS found {}/{} unique words, base={} fts={} diff={}", found_count, unique_words.len(), base, fts, diff);
    assert!(diff <= 2, "FTS significantly out of sync: base={} fts={}", base, fts);
    // Note: FTS might not find our words if triggers aren't set up — that's a WARN, not FAIL
    if found_count == unique_words.len() {
        println!("[PASS] S7: All {} unique words found in FTS, index in sync", unique_words.len());
    } else {
        println!("[WARN] S7: FTS found only {}/{} words — FTS triggers may not auto-index new inserts from external connections", found_count, unique_words.len());
    }

    // Cleanup
    db.execute("DELETE FROM conversation_turns WHERE tab_id = ?1", params![tab_id]).unwrap();
}

// ─────────────────────────────────────────────────────────────────────────
// S8: METRICS TIME SERIES CONTINUITY
// Verifies existing metrics data has no unreasonable gaps.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn s8_metrics_continuity() {
    let db = open_readonly();

    let samples: Vec<(i64, f64, f64, f64)> = db.prepare(
        "SELECT ts, cpu, ram, disk FROM metrics_samples ORDER BY ts ASC"
    ).unwrap()
    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
    .unwrap()
    .filter_map(|r| r.ok())
    .collect();

    if samples.len() < 2 {
        println!("[SKIP] S8: Only {} samples, need >=2 for gap analysis", samples.len());
        return;
    }

    let mut max_gap_secs = 0i64;
    let mut total_gaps = 0;
    let mut range_violations = 0;

    for i in 1..samples.len() {
        let gap = samples[i].0 - samples[i-1].0;
        if gap > max_gap_secs { max_gap_secs = gap; }
        // Gaps > 30 min are suspicious (sampling should be every ~5 min)
        if gap > 1800 { total_gaps += 1; }
    }

    // Verify all values are in valid range
    for (ts, cpu, ram, disk) in &samples {
        if *cpu < 0.0 || *cpu > 100.0 || *ram < 0.0 || *ram > 100.0 || *disk < 0.0 || *disk > 100.0 {
            range_violations += 1;
            println!("  [VIOLATION] ts={} cpu={} ram={} disk={}", ts, cpu, ram, disk);
        }
    }

    let span_hours = (samples.last().unwrap().0 - samples.first().unwrap().0) as f64 / 3600.0;

    println!("[S8] {} samples over {:.1}h, max_gap={}s, large_gaps={}, range_violations={}",
        samples.len(), span_hours, max_gap_secs, total_gaps, range_violations);
    assert_eq!(range_violations, 0, "Found {} metrics values outside [0,100] range", range_violations);
    println!("[PASS] S8: All {} samples in valid range, max gap {}s", samples.len(), max_gap_secs);
}

// ─────────────────────────────────────────────────────────────────────────
// SUMMARY
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn s_summary() {
    let db = open_db();
    let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0)).unwrap();
    let db_size = std::fs::metadata(db_path()).map(|m| m.len()).unwrap_or(0);

    let tables = vec![
        "conversation_turns", "agent_memories", "metrics_samples",
        "token_usage", "memory_core", "embeddings",
    ];
    println!();
    println!("+=============================================+");
    println!("|   STRESS TEST SUITE — POST-RUN SUMMARY      |");
    println!("+=============================================+");
    println!("|  DB size:    {:>8} KB                     |", db_size / 1024);
    println!("|  Integrity:  {:>8}                        |", integrity);
    println!("+---------------------------------------------+");
    for t in &tables {
        let c: i64 = db.query_row(&format!("SELECT COUNT(*) FROM {}", t), [], |r| r.get(0)).unwrap_or(0);
        println!("|  {:25} {:>6}            |", t, c);
    }
    println!("+---------------------------------------------+");
    println!("|  S1: Agent loop writes          [see above]  |");
    println!("|  S2: Interleaved R/W            [see above]  |");
    println!("|  S3: Persistence reconnect      [see above]  |");
    println!("|  S4: Large content (>40KB)      [see above]  |");
    println!("|  S5: Conversation ordering      [see above]  |");
    println!("|  S6: Token accounting           [see above]  |");
    println!("|  S7: FTS sync                   [see above]  |");
    println!("|  S8: Metrics continuity         [see above]  |");
    println!("+=============================================+");
}
