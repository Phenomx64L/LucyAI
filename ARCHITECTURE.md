# Lucy — Architecture & File-Root Map (for deep error scanning)

> Generated v1.7.208 · 2026-06-22. Purpose: a navigable map of every subsystem
> and its files, ranked by size/criticality, so a deep bug scan has a root to
> start from. Line counts are LOC at generation time — they flag *where the
> risk concentrates*, not exact current values.
>
> Companion docs: `DESIGN.md` (conceptual design), `CHANGELOG.md` (every fix
> carries its symptom in the subject), `INSTALLER.md`, `SETUP.md`.

---

## 1. Stack & layers

```
┌──────────────────────────────────────────────────────────────────────┐
│  FRONTEND  — SvelteKit (Svelte 5 legacy `$:` mode) · TypeScript + JS   │
│  src/                                                                  │
│   routes/+page.svelte ........ the monolith shell + AGENT LOOP (13.4k) │
│   lib/  ...................... 195 components + TS modules             │
│                                                                        │
│   ↕ Tauri IPC  (invoke / emit)  — typed bindings in src/lib/types/     │
│                                                                        │
│  BACKEND  — Rust (Tauri 2) · src-tauri/src/                            │
│   lib.rs (entry/run) → commands/* (71 modules) → utils/* + guardrails/*│
│                                                                        │
│   ↕ rusqlite (r2d2 pool, WAL) + sqlite-vec (ANN)                       │
│                                                                        │
│  DATA  — SQLite `lucy.db` @ %APPDATA%\com.lucy.dev\                    │
│   agent_memories · memory_crystals · embeddings(vec) · runbooks ·      │
│   agent_session_summaries · audit_trail · incidents · metrics         │
│                                                                        │
│  EXTERNAL — Anthropic / Gemini / Ollama (LLM + embeddings) ·           │
│             MCP servers · WebView2 (Chromium) renderer                 │
└──────────────────────────────────────────────────────────────────────┘
```

Build/version touch-points (bump together): `package.json`,
`src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `CHANGELOG.md`.
Pre-commit hook gate: ~331 Rust tests + `cargo check` + `svelte-check` (0 err) +
246 vitest.

---

## 2. Backend — `src-tauri/src/` (45k LOC, 71 command modules)

Entry: `main.rs` → `lib.rs` (1131 — WebView2 flags, Tokio runtime, command
registration, migrations). Shared: `state.rs` (294), `utils/`, `guardrails/`.

### 2.1 LLM / AI core
| File | LOC | Role |
|---|---|---|
| `commands/ai.rs` | 1889 | LLM calls, `fetch_url` (SSRF guard), Anthropic/Gemini payloads, model resolver |
| `commands/prompt_sections.rs` | 1409 | System-prompt assembly (sections, budget) |
| `commands/providers.rs` | 214 | Provider/key status (key value never crosses IPC) |
| `commands/reflection.rs` | 316 | Self-reflection / verifier passes |
| `commands/smart_chips.rs` | 664 | Chip generation/classification |

### 2.2 Execution (HIGH-RISK — security-sensitive)
| File | LOC | Role |
|---|---|---|
| `commands/local.rs` | 2175 | CMD exec, file ops, tasklist; `pdf_ingest` path validation |
| `commands/shell.rs` | 973 | **PowerShell exec — CRITICAL.** No-output bug history; contract tests |
| `utils/shell.rs` | 305 | Shell helpers (the permanent fix lives here) |
| `commands/pty.rs` | 463 | PTY sessions |
| `commands/script_verify.rs` | 383 | Pre-exec static script checks |
| `commands/sandbox_preview.rs` | 329 | Dry-run preview |

### 2.3 Memory / RAG pipeline (Tier 1–3)
| File | LOC | Role |
|---|---|---|
| `commands/metrics.rs` | **4039** | `save_agent_memory` (2-stage dedup), metrics, sessions — BIGGEST backend file |
| `commands/memory.rs` | 1273 | Recall, decay (`DECAY_INJECT_THRESHOLD`), injection gates |
| `commands/embeddings.rs` | 767 | Ollama/Gemini embeddings (fire-and-forget, silent skip) |
| `commands/vec_search.rs` / `vec_index.rs` | 540/301 | sqlite-vec ANN search + index |
| `commands/reranker.rs` | 247 | Cross-encoder rerank (ml-reranker feature) |
| `commands/grounding.rs` | 462 | Memory grounding |
| `commands/chip_memory.rs` | 556 | Chip → memory linkage |
| `commands/auto_dedup.rs` | 274 | Dedup automation |
| `commands/knowledge_graph.rs` | 561 | KG nodes/edges |
| `commands/synonyms.rs` | 206 | Synonym expansion |

### 2.4 Memory science (kappa-graph — ADR-driven, `docs/research/kappa-graph/`)
`annealing.rs` (603) · `polarity.rs` (330) · `causal.rs` (271)

### 2.5 Incident / diagnostics / observability
`incident.rs` (803) · `diagnostics.rs` (926) · `log_analysis.rs` (457) ·
`process_lineage.rs` (443) · `self_healing.rs` (299) · `proactive_detector.rs`
(414) · `capacity.rs` (546) · `daily_patterns.rs` (254) · `causal.rs` ·
`incident_detective` · `frontier_telemetry` · `activity_feed.rs` (298)

### 2.6 Security & integrity (HIGH-RISK)
| File | LOC | Role |
|---|---|---|
| `commands/security_skills.rs` | 1209 | Bundled+user security skill catalog |
| `commands/threat_scan.rs` | 494 | Threat scanning |
| `commands/cve_match.rs` | 366 | CVE matching |
| `guardrails/scanner.rs` | 468 | Output/command scanner |
| `guardrails/prompt_guard.rs` | 346 | Prompt-injection guard |
| `commands/audit.rs` | 329 | Audit trail (secret-scrubbed) |
| `utils/secret_scrubber.rs` | 221 | PII/secret scrubbing (save + audit) |
| `utils/placeholder_guard.rs` | 207 | Placeholder/credential guard |
| `hash_chain` | — | Tamper-evident audit chaining |

### 2.7 Hosts / remote / computer-use
`hosts.rs` (715, SSH key_path injection-guarded) · `rdp_agent.rs` (644) ·
`shell_recording.rs` (279) · `local_screen.rs` (468) ·
`commands/computer_use/` → `mod.rs`, `traits.rs`, `types.rs`,
`anthropic.rs`, `gemini.rs`, `openai.rs`, `ollama.rs` (358)

### 2.8 MCP / integrations
`mcp.rs` (1246, budget guard) · `dashboard_integrations.rs` (319) ·
`object_bridge.rs` (469) · `notify`

### 2.9 Infra / persistence / housekeeping
`utils/db.rs` (912, r2d2 pool/WAL/migrations) · `utils/simd_cosine.rs` (529,
SIMD hot path) · `housekeeping.rs` (1174) · `db_backup.rs` (618) ·
`db_maintenance.rs` (354) · `state_snapshot.rs` (402) · `support_bundle.rs`
(334) · `scheduled.rs` (418) · `replay.rs` (382) · `state.rs` (294)

### 2.10 Docs / inventory / misc
`pdf.rs` (526, pdf-extract, no OCR) · `inventory.rs` (226) ·
`inventory_drift.rs` (467) · `compliance` · `runbook_gen.rs` (369) ·
`fork_advisor.rs` (518) · `ui.rs` (301) · `config.rs` (213) · `system.rs` (335)

---

## 3. Frontend — `src/` (74.8k LOC)

### 3.1 The monolith (HIGHEST-RISK — start here)
| File | LOC | Role |
|---|---|---|
| `routes/+page.svelte` | **13370** | App shell + **agent loop (`runAI`)** + streaming pipeline + reasoning bubble. Has a null byte ~offset 264909 → use `grep -a` for full-file search |
| `lib/NexShellView.svelte` | 4155 | Remote/local shell terminal; per-shell input drafts live in the `nsInput` map (paint bug fixed v1.7.221) |
| `lib/page/slash-commands.ts` | 2943 | Slash-command dispatch table |

### 3.2 Extracted page logic — `src/lib/page/` (11 files) & `hooks/`
`tabs-store.ts` · `workspace-presets.ts` · `agent-checkpoints.ts` ·
`chips-quick-actions.ts` · `fix-store.ts` · `host-preflight.ts` ·
`mcp-secrets.ts` · `ql-popover.ts` · `slash-commands.ts`
`hooks/turn-loop.ts` (336) · `hooks/command-guard.ts` (372)

### 3.3 Chat & streaming render (FRAGILE — repeated bug source)
`ChatThread.svelte` (865, reasoning bubble + msg roles) · `ChatInput.svelte`
(697, `_draft` paint-safe input) · `ChatEmptyState.svelte` (287) ·
`message-render.ts` (509) · `llm-stream.ts` (455, shadowed copy — real
`askLucyStream` is local in +page.svelte) · `stream-parse.ts`
(`makeThoughtStreamer`) · `morph-html.ts` (morphdom diff + pin-scroll) ·
`AgentChapterView.svelte` (490)

### 3.4 Model routing / providers
`smart-router.ts` (737) · `models.js` (247) · `llm-models.ts` ·
`model-routing.ts` · `provider-fallback.ts` · `tier-health.ts` (417) ·
`ModelSwitcherChip.svelte` (271) · `ProviderConfigModal.svelte` (1082)

### 3.5 Memory (frontend)
`MemoryBrowserView.svelte` (1992) · `MemoryGraphView.svelte` (1540) ·
`KgMiniViewer.svelte` (271) · `MemoryFeed.svelte` (288) ·
`unified-context.ts` (398) · `context-compressor.ts` · `agentmemory/`
(`verify.ts` 434, `patterns.ts` 355)

### 3.6 Dashboards / views
`DashboardView.svelte` (1419) · `CostDashboardView.svelte` (1104) ·
`InventoryView.svelte` (619) · `ComplianceView.svelte` (294) ·
`LogViewerView.svelte` (388) · `LogTimelineView.svelte` (463) ·
`ReplayBrowserView.svelte` (531) · `SelfDiagnosticsView.svelte` (303) ·
`AuditTrailView.svelte` (323)

### 3.7 Skills (4 distinct surfaces — see §5 gotcha)
`skills/skill-engine.ts` · `skills/builtin/index.ts` (308) ·
`skill-presets.ts` (947) · `skill-factory.ts` (253) ·
`SkillBrowserModal` · `SkillsManagerModal.svelte` (1250, **DEAD/retired**) ·
`SkillPicker.svelte` (444) · `SkillPresetPicker.svelte` (308) ·
`SkillCatalogModal.svelte` (331)

### 3.8 Modals / panels / chrome
`ProviderConfigModal` (1082) · `PermissionRulesModal` (873) ·
`McpServersModal` (865) · `HostModal` (576) · `IncidentPanel` (567) ·
`Sidebar` (566) · `StatusBar` (670) · `TabBar` (647) · `SetupOverlay` (572) ·
`TutorialOverlay` (910) · `LiveTracePanel.svelte` (340, Alt+T live telemetry)

### 3.9 Pure-logic libs (most have `.test.ts` — lower risk)
`stores.ts` (379) · `lucy-api.ts` (335) · `input-classifier.ts` (297) ·
`predictive-chips.ts` (441) · `command-signatures.ts` (432) ·
`output-enricher.ts` (405) · `script-verifier.ts` (334) ·
`anomaly-bridge.ts` (295) · `design-md.ts` (295) · `notebook.ts` (354) ·
plus the refactor-extracted tested modules: `auto-promote.ts`,
`plan-detect.ts`, `tab-budget.ts`, `tool-result-classify.ts`,
`provider-fallback.ts`, `artifacts.ts`, `model-routing.ts`, `text-utils.ts`

### 3.10 Styles — `src/lib/styles/` (8 files) + `routes/page.css`
See §5 CSS gotcha: real chat bubbles live in `ChatThread.svelte`'s scoped
`<style>`, which WINS over `styles/chat-thread.css`.

---

## 4. Cross-cutting concerns (scan these as *flows*, not single files)

| Concern | Touches |
|---|---|
| **Agent loop / turn progression** | `+page.svelte::runAI`, `hooks/turn-loop.ts`, `agent-loop-util.ts`, reasoning bubble (6122+), skip-stuck |
| **Streaming render** | local `askLucyStream` (+page.svelte), `stream-parse.ts`, token queue/drain timer, `morph-html.ts`, `ChatThread.svelte`, WebView2 flags (`lib.rs`) |
| **Memory save/recall gates** | `metrics.rs` (dedup), `memory.rs` (decay/inject), `embeddings.rs` (silent skip), `vec_search.rs` |
| **Security / HITL** | `guardrails/*`, `auto-promote.ts` deny-list, `command-guard.ts`, `secret_scrubber.rs`, SSRF in `ai.rs`, prefill-not-autoexec |
| **IPC contract** | `src/lib/types/*.ts` ↔ `#[derive(ts_rs::TS)]` structs (regen via `cargo test export_bindings`) |

---

## 5. Known gotchas (don't re-discover — scan with these in mind)

1. **`+page.svelte` null byte** ~offset 264909 → ripgrep treats as binary; use
   `grep -a` (Bash) for full-file text search.
2. **Two `askLucyStream`**: the real one is LOCAL in `+page.svelte`; the
   `llm-stream.ts` copy is shadowed/dead for chat. Patch the local one.
3. **CSS bubble override**: chat bubble styles in `ChatThread.svelte` scoped
   `<style>` beat `styles/chat-thread.css`. Edit the scoped block.
4. **Paint-starvation inputs**: never `bind:value={item.field}` on an `{#each}`
   member for high-freq inputs (invalidates whole array → paint death). Use the
   `_draft` pattern (`ChatInput.svelte`), or a separate map keyed by id.
   **NexShellView was FIXED in v1.7.221** — its five inputs (`directIn`,
   `lucyIn`, `interactiveInput`, `rdpResultIn`, `rdpAgentTask`) moved off the
   session object into the `nsInput` map, so typing no longer invalidates
   `rshellSessions`. This entry used to say the bug was still there; it sent at
   least one reader to re-fix work already done. The rule stands, the example
   does not.
5. **Memory "doesn't save/recall"**: 2-stage dedup collapse + decay threshold +
   embeddings silent-skip — behavior, not crash. `/memory-health` to diagnose.
6. **manual_clamp clippy (21×)** are intentional (panic-safe) — not bugs.
7. **Skills = 4 different surfaces** with similar names (see memory
   `architecture-navigation`); `SkillsManagerModal` is dead code.

---

## 6. Suggested deep-scan order (highest error-density first)

1. `src-tauri/src/commands/metrics.rs` (4039) — dedup/race/SQL correctness
2. `src/routes/+page.svelte` (13370) — agent loop, streaming, reasoning, state
3. `src/lib/NexShellView.svelte` (4155) — shell flows, session lifecycle
4. `src-tauri/src/commands/local.rs` (2175) + `shell.rs` (973) — exec safety
5. `src/lib/page/slash-commands.ts` (2943) — dispatch correctness
6. `commands/ai.rs` (1889) + `prompt_sections.rs` (1409) — LLM payloads, SSRF
7. `commands/memory.rs` / `embeddings.rs` / `vec_search.rs` — RAG gates
8. `guardrails/*` + `secret_scrubber.rs` — security invariants
9. Streaming flow end-to-end (see §4) — the recurring freeze/clip area
10. Sweep the `.test.ts`-backed pure libs last (lowest risk)

Tooling for the scan: `cargo clippy --all-targets`, `cargo test`,
`npm run check` (svelte-check), `npx vitest run`, `cargo audit`, `npm audit`.
Known-noise advisories are catalogued in memory `integrity-audit-verdict`
(don't re-investigate those).
