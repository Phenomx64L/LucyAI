pub mod ai;
pub mod compliance;
pub mod computer_use;
pub mod config;
pub mod hosts;
pub mod inventory;
pub mod local;
pub mod logs;
pub mod metrics;
pub mod providers;
pub mod prompt_sections;
pub mod rdp_agent;
pub mod shell;
pub mod system;
pub mod ui;
pub mod indexer;
pub mod mcp;
pub mod incident;
pub mod embeddings;
pub mod memory;
pub mod pdf;
pub mod principles;
pub mod reflection;
pub mod scheduled;
pub mod synonyms;
pub mod dedup;
pub mod reranker;
pub mod vec_index;
pub mod audit;
pub mod capacity;
pub mod diagnostics;
pub mod notify;
pub mod log_analysis;
pub mod state_snapshot;
pub mod process_lineage;
pub mod self_healing;
pub mod causal;
pub mod threat_scan;
pub mod object_bridge;
pub mod runbook_gen;
pub mod daily_patterns;
pub mod sandbox_preview;
pub mod knowledge_graph;
pub mod incident_detective;
pub mod frontier_telemetry;
pub mod activity_feed;
pub mod replay;
pub mod shell_recording;
pub mod cve_match;
pub mod db_backup;
pub mod support_bundle;
pub mod inventory_drift;
pub mod dashboard_integrations;
pub mod hash_chain;
pub mod smart_chips;
pub mod chip_memory;
pub mod db_maintenance;
// v1.6.0 — probabilistic truth convergence (Kappa Graph ADR-044).
pub mod grounding;
// v1.6.5 — polarity axis triangulation (Kappa Graph ADR-058).
pub mod polarity;
// v1.6.6 — annealing ontologies MVP (Kappa Graph ADR-200).
pub mod annealing;
// v1.7.4 — Anthropic Cybersecurity Skills library (213 SKILL.md, Apache 2.0).
pub mod security_skills;
// v1.7.16 — Pre-delivery script syntax verification.
pub mod script_verify;
// v1.7.73 — Auto-fork heuristic. Scores user prompts for parallel-branch
// suitability and nudges the LLM toward fork_task / wait_task when ≥2
// independent investigations are detected.
pub mod fork_advisor;
// v1.7.80 — Proactive Operations Assistant. Background detectors that
// surface operator-actionable insights without being asked
// (memory buildup, log size, DB size, integrity alarms, failure spikes).
pub mod proactive_detector;
