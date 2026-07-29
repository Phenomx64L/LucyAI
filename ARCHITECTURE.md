# Lucy — Architecture & File-Root Map (for deep error scanning)

> Generated v1.7.208 · 2026-06-22 · revised v1.8.1 · 2026-07-28: new §4.1
> attachment pipeline, §4.2 auto-execution gate, §4.3 agent-loop context, §7 CI
> and supply chain, and gotchas 8-14. Purpose: a navigable map of every
> subsystem and its files, ranked by size/criticality, so a deep bug scan has a
> root to start from. Line counts are LOC at generation time — they flag *where
> the risk concentrates*, not exact current values.
>
> Cross-instance work: `docs/COLLABORATION.md` is the operating model and
> `docs/HANDOFF.md` the live transfer. The agent's own memory does NOT cross
> systems — what is not written in those two files is lost at the turn boundary.
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
| **Agent loop / turn progression** | `+page.svelte::runAI`, `hooks/turn-loop.ts`, `agent-loop-util.ts`, reasoning bubble (6122+), skip-stuck — see §4.3 |
| **Streaming render** | local `askLucyStream` (+page.svelte), `stream-parse.ts`, token queue/drain timer, `morph-html.ts`, `ChatThread.svelte`, WebView2 flags (`lib.rs`) |
| **Memory save/recall gates** | `metrics.rs` (dedup), `memory.rs` (decay/inject), `embeddings.rs` (silent skip), `vec_search.rs` |
| **Security / HITL** | `guardrails/*`, `auto-promote.ts` deny-list, `command-guard.ts`, `secret_scrubber.rs`, SSRF in `ai.rs`, prefill-not-autoexec — see §4.2 |
| **IPC contract** | `src/lib/types/*.ts` ↔ `#[derive(ts_rs::TS)]` structs (regen via `cargo test export_bindings`) |
| **File attachments** | `ui.rs::pick_multiple_files` / `pdf.rs::extract_pdf_text*` → `file-inputs.ts` → `process()` → cockpit mirror → prompt builder — see §4.1 |

### 4.1 Attachment pipeline (v1.8.1 — end to end)

Four hops, and the contract that ties them together is the **mime → `type`
mapping**. Get that wrong and the file silently never reaches the model.

```
 ①  INGEST            ②  CLASSIFY           ③  COMPOSE          ④  RENDER / SEND
 picker  ─┐                                 process()           cockpit bubble
 (ui.rs)  ├─ (name, content, mime) ─→ file-inputs.ts ─→ t.attachedFiles ─┬─→ msg.attachments
 drop ────┘                            type: image|text                  └─→ ctx '--- ARCHIVOS ---'
```

**① Ingest — two entry points that must stay in sync.**
`ui.rs::pick_multiple_files` (clip button) reads from a real path, so PDFs are
extracted there via `pdf.rs::extract_pdf_text`. Drag-and-drop cannot use it:
`tauri.conf.json` sets `dragDropEnabled: false`, so drops arrive as HTML5 `File`
objects with **no filesystem path** — those go through
`pdf.rs::extract_pdf_text_from_bytes` (base64 → temp file → same extractor).
Both extraction paths share `pdf_ingest`'s markitdown → `pdf-extract` chain, so
an attached PDF and an ingested PDF yield identical text.

**② Classify — the contract.** `mime` decides `type`, and `type` is what every
later stage dispatches on:

| mime | `type` | `content` holds | Consumed by |
|---|---|---|---|
| `image/*` | `image` | base64 | vision payload (`imgs`) |
| `application/pdf` | **`text`** | already-extracted text | `--- ARCHIVOS ---` |
| anything else | `text` | file text | `--- ARCHIVOS ---` |
| `__error__` | — | error message | toast; not attached |

`type` has exactly **two** legal values. Adding a third (`'pdf'`, `'doc'`) drops
those files from the prompt builder's `filter(f => f.type === 'text')` — that is
precisely the v1.8.0 bug.

**③ Compose.** `process()` (`+page.svelte`) must pass **`rawContent`** — the
clean user text — alongside `html`. It also passes ALL attachments as
`{name, kind, previewUrl?, chars?}`.

**④ Render / send.** The cockpit renders images as thumbnails and documents as
`.msg-doc` chips. The prompt builder appends text files under
`--- ARCHIVOS ---` and pushes images into the vision array.

Regression nets (v1.8.1, extended 2026-07-28):

- `src/lib/file-inputs.test.ts` — the mime → `type` table for the picker, plus
  the whole drop path. The load-bearing one is **"starts every read before
  yielding to the event loop"**: it models Chromium's data-store teardown with
  a fake `FileReader` (a read kicked off while the store is alive completes;
  one started after rejects with `NotFoundError`), so the gotcha-11 timing
  invariant fails loudly. Verified by mutation — inserting a single
  `await Promise.resolve()` above the read fails exactly 3 tests, where before
  it passed all four gates.
- `pdf.rs::tests` — the extractor itself. `pdf_extract_reads_a_real_text_layer_pdf`
  builds a valid one-page PDF (xref offsets computed, not hand-written) and
  calls `pdf_extract::extract_text` **directly**, on purpose: the wrapper tries
  markitdown first, so a wrapper-level test would pass through markitdown on a
  machine that has it and mask a broken pure-Rust fallback — which is the path
  every machine without markitdown uses. Re-run after any `pdf-extract`/`lopdf`
  move; §7 says to expect them.
- The drop-side backend (`extract_pdf_text_from_bytes`) pins its guards: size
  ceiling checked before decoding, invalid base64, filename-is-cosmetic, and
  temp-file cleanup on both the success and failure paths.

**Cleanup ordering, `extract_pdf_text_from_bytes`:** the staged temp file is
deleted BEFORE the join result is unwrapped. `pdf-extract` panics on some
malformed PDFs, and a panic arrives as a `JoinError` — the earlier `?` on the
joined result returned first and left the user's document bytes in `%TEMP%`
indefinitely. "Cleanup on every exit path" has to mean every path, not just
the ones that return a value.

### 4.2 Auto-execution gate — the four layers, and where they leaked

A model-emitted command reaches PowerShell through four gates. v1.8.1 closed a
hole that went through **all four**:

```
LLM output → auto-promote.ts (frontend, no human) → execute_powershell IPC
             → obfstr blocklist → DESTRUCTIVE_VERB_RE → guardrails::scan → run
```

`Start-Process` is allow-listed in `auto-promote.ts` (it is what "ábrelo" means),
and the deny-list only knew the fully spelled-out `-EncodedCommand`. **PowerShell
resolves any unambiguous prefix of a parameter name**, so `-e`, `-en`, `-enc` all
mean the same thing — and the Rust blocklist substring-matches the literal
`"-encodedcommand"`, which `-enc` never contains. `DESTRUCTIVE_VERB_RE` does not
cover `Start-Process`, and the guardrail bank only flags `Start-Process … -Verb
RunAs`. Net effect: `Start-Process powershell -enc <base64>` **auto-executed with
no human in the loop** — one injected line in any file Lucy read was RCE.

Fixed on both sides, because **the frontend is not a security boundary**: any
caller reaching the `execute_powershell` IPC command directly skips it.
`ENCODED_CMD_RE` (nested optional groups spelling every prefix of
`encodedcommand`, `\b`-anchored so `-Encoding`/`-ErrorAction`/`-ea` do not match)
lives in BOTH `auto-promote.ts` and `shell.rs`. `LAUNCH_ABUSE_RE` additionally
refuses launchers pointed at interpreters/LOLBins, executable extensions,
`-ArgumentList`, or UNC paths — while still promoting `Start-Process "report.pdf"`.

Regression nets: `src/lib/auto-promote.test.ts` and `shell.rs::tests`
(`encoded_cmd_backstop_*`). Both pin the false-positive cases too — over-blocking
here costs a confirmation prompt, under-blocking costs a shell.

### 4.3 Agent-loop context: three independent shrinkers (v1.8.1)

Three mechanisms cut context, at different scopes. They interact, and the
interaction is where the bugs live.

| # | Mechanism | Scope | Persists? |
|---|---|---|---|
| 1 | **Rolling window** (`AGENT_CTX_ROLLING_MAX` 35 kB, keeps last 5 tool blocks) | `agentCtx`, per loop turn | **yes** — rewrites `agentCtx` |
| 2 | **`compressContext`** (Ph1 local dedup free · Ph2 LLM, gated) | per continuation call | **no** — `compressedCtx` is used for the call and dropped |
| 3 | **Tab compaction** (`pruneTabForBudget` 60 k tok + `compaction.keepFrom` + `contextMax` 50 kB) | `t.messages` → HISTORIAL, between user turns | **yes** |

Consequences worth knowing before touching any of them:

- **#2 recomputes every turn by design.** `agentCtx` is the running record; the
  compacted form is a per-call view. It looks wasteful in the trace (identical
  "Context compacted 33.8k → 25k" lines every turn) but Phase 2 — the only part
  that spends an LLM call — is gated by `_lastPhase2InputLen`, so the repeated
  work is the free local dedup. **Trace volume here is a symptom of a long loop,
  not its cost.**
- **#1 used to defeat the stall detector.** See gotcha 13.
- **#3 evicts the biggest message first**, which is always the deliverable. See
  gotcha 14.

Anti-grind guards (`_STALL_LIMIT` 3 · `_ESCALATE_AFTER` 2 · `_EMPTY_GUARD_BAIL`
3 · `MAX_IDENTICAL_TOOL_CALLS` 3 · `_intentOnlyStreak`) are all **consecutive**
streaks. A failure that alternates with partial successes trips none of them and
rides to `MAX_LOOPS` (60).

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
8. **Attachment `type` is a two-value enum** — `'image' | 'text'`, nothing else
   (§4.1). A PDF is `type: 'text'` carrying `mimeType: 'application/pdf'`,
   because `content` already holds its EXTRACTED TEXT. Until v1.8.1 `attach()`
   read "not `text/plain`" as "image", so PDFs became fake images and the
   prompt builder's `filter(f => f.type === 'text')` skipped them: the composer
   showed a chip, the model got nothing, and the symptom looked like "Lucy
   can't read my PDFs" — users worked around it by pasting absolute paths.
   Branch on `mimeType` for ICONS, never on `type`.
9. **Cockpit mirror: pass `rawContent`, not just `html`.** `addMsg` mirrors user
   messages into the cockpit with
   `rawContent ?? content ?? stripTags(html)`. Any call site that supplies only
   `html` gets its markup flattened into the bubble — which is how
   `<div class="mn">Iván</div>` plus an inline `Archivos: · x.pdf` span rendered
   as the single run-on line *"Iván mi pregunta Archivos: · x.pdf"*. The `html`
   field is for the legacy V1 chat view; the cockpit wants clean text plus
   structured `attachments`.
10. **Two drop handlers exist** — `onDrop` (global overlay) and
   `handleFileDrop` (per-tab). They drifted apart once already; both now
   delegate to `readDroppedFile`. Change the shared reader, not one caller.
11. **Dropped files must be read SYNCHRONOUSLY.** Chromium/WebView2 tears the
   drag data store down when the drop handler returns, and the `File` objects
   stop being readable (`NotFoundError: A requested file or directory could not
   be found…`). `onDrop` in `+page.svelte` used to hand the event to the file
   reader from inside `maybeInstallSkillFromDrop(e).then(…)` — always too late
   — and the old readers had no `onerror`, so the drop silently did nothing.
   Always call `startReadingDrop(e.dataTransfer)` first, await later.
12. **The stall detector measures REAL growth, not net size.** `_noGrowthStreak`
   compares the effective context length turn over turn, and a negative delta
   resets it ("it shrank, so a digest ran, so there was progress"). But the
   rolling window (§4.3 #1) also shrinks it, and that is pure bookkeeping. With
   the window firing every 4-6 turns and `_STALL_LIMIT` at 3, the streak was
   wiped before it could ever reach the limit — a grinding run rode 24+/60 turns
   with every stall signal erased by the window it had itself triggered. Fixed
   in v1.8.1 by adding `_rollingDroppedThisTurn` back into the delta. **If you
   add a fourth shrinker, it must feed that adjustment too.**
13. **A delivered artifact must be anchored outside the compactable window.**
   The HISTORIAL is rebuilt from `t.messages` under two cuts (`keepFrom` and
   `contextMax`), and a generated report is the largest message, so it is the
   first evicted — by the very compaction its own long run triggered. The user
   then asks Lucy to act on the report she just wrote and gets "I have no report
   loaded in the context of our conversation", which is literally true and reads
   as amnesia. `renderAgentTask` now stores the last substantial output on the
   TAB (`t._lastDeliverable`, ≥600 chars), and the context builder re-injects it
   via `buildDeliverableAnchor` (`$lib/deliverable-anchor.ts`, tested),
   reserving its budget BEFORE the history walk so it displaces old turns
   instead of overflowing `contextMax`.
14. **`npm run check` does NOT type-check `+page.svelte`.** `jsconfig.json` sets
   `"checkJs": false`, so the 14.5 kLOC monolith — agent loop included — is
   unchecked JS: undefined identifiers, wrong arity, everything passes. The
   "0 errors" badge only covers `.ts`/`.svelte` files with typed script blocks.
   **When editing `+page.svelte`, verify every identifier is in lexical scope —
   no tool will tell you.**

   Reproduce the real error count with a throwaway config (do NOT flip the
   committed one — CI would go red on day one):

   ```json
   // jsconfig.scan.json — delete after use
   { "extends": "./jsconfig.json",
     "compilerOptions": { "checkJs": true, "strict": false } }
   ```
   ```
   npx svelte-check --tsconfig ./jsconfig.scan.json --output machine
   ```

   Measured 2026-07-28: 241 errors / 36 files, 25 `Cannot find name`. Five were
   real `ReferenceError`s on `esc` (a local alias of `escapeHtml` declared
   ~2000 lines below its use), crashing `/pantalla`'s error path and all of
   `/controlar`.

   **Triage of the remaining 20 completed 2026-07-28 (now 228 / 12).** The
   pattern worth carrying forward: `typeof x === 'undefined'` guards are SAFE
   on an undeclared identifier, a bare reference is not — and a bare one as the
   FIRST statement of a `try` kills every statement after it.

   | Identifier | Verdict |
   |---|---|
   | `listStaleCheckpoints` | **Real.** Imported aliased as `listStaleCkpts`; the unaliased call threw on entry, so interrupted-agent checkpoint recovery never ran once. Fixed. |
   | `_runToken` | **Real.** Declared nowhere in the repo. The bare reference opened the tab-close cleanup `try`, so `_forkBypassByTab` / `_forkAdviceByTab` / `_lastTitledTurn` were never reclaimed — the leak-fix block leaked. Removed; `t._cancelled` already does the invalidation its comment described. |
   | `destroyEnrichedWidgets` | **Real.** Exported by `$lib/message-render` but never imported here, so the widget teardown on tab close silently no-op'd into the detached-DOM leak it existed to prevent. Fixed by importing it. |
   | `loop_i` (×8) | Benign, degraded. All `typeof`-guarded; it is the `for (let loop_i…)` variable at ~7523, out of scope in these callers. Cost: trace events always report `iteration: null`, and `hitLimit` is permanently false so the "límite de iteraciones con errores" fallback can never be chosen. Fixing means threading it through the agent loop — hot zone, deliberate call. |
   | `aiParams` (×4) | Benign, degraded. `const` from a different block; `typeof`-guarded, falls back to `getEffectiveModel(t)`. Diverges only when the loop routed to `_routedLoopModel`, in which case the provider-fallback notice names the wrong model. |

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
`npm run check` (svelte-check — but read gotcha 14 first), `npx vitest run`,
`cargo audit`, `npm audit`.

---

## 7. CI and supply chain (v1.8.1)

`.github/workflows/ci.yml` mirrors `.githooks/pre-commit` so `--no-verify`
cannot land what the hook would have caught, and adds the two checks the hook is
too short for. Three jobs:

| Job | Runner | Gate |
|---|---|---|
| `frontend` | ubuntu | svelte-check · vitest · `npm run build` · `npm audit --audit-level=high` |
| `backend` | **windows** | `cargo check` · `cargo test --lib -- --test-threads=1` · clippy |
| `audit` | ubuntu | `cargo audit` |

Three constraints that are **load-bearing** — changing them silently breaks the
gate rather than failing loudly:

1. **`backend` must stay on Windows.** The crate is Windows-only (`winapi`,
   `winreg`, `std::os::windows`) and the `shell.rs` contract tests spawn real
   `powershell.exe`. Linux cannot run them at all.
2. **Clippy denies only `correctness` + `suspicious`.** The style/complexity
   groups carry ~101 known warnings (21 `manual_clamp` are intentional, §5 #6);
   denying everything would make CI red on day one.
3. **`src-tauri/target` is NOT cached** — measured 14.5 GB against GitHub's
   10 GB per-repo budget. Caching it fails and evicts everything else. Only the
   cargo registry is cached.

### Advisory triage — `src-tauri/.cargo/audit.toml`

`cargo audit` fails on any advisory NOT on the written ignore list, and every
entry there carries a reachability verdict. Current state (triaged 2026-07-28):

- **Fixed**: `lopdf` (RUSTSEC-2026-0187, **7.5 high** — stack overflow on nested
  PDF objects, directly reachable through attachments/`pdf_ingest`; escaped via
  `pdf-extract` 0.7 → 0.12, which pulls lopdf ≥ 0.42) and `crossbeam-epoch`.
- **Ignored, upstream pins Lucy cannot move**: `quick-xml` ×2 (via `plist` ←
  `tauri-utils`, and `tauri-winrt-notification` ← `notify-rust`), `sqlx` (pinned
  by `tauri-plugin-sql`; the advisory concerns the Postgres/MySQL binary
  protocols and Lucy's path is SQLite through rusqlite), `rsa` (no fix exists;
  arrives via sqlx's MySQL auth, never executed).

**Re-triage on every Tauri upgrade** — most of these resolve themselves upstream.
One caveat if you build on a non-Windows host: the `informational_warnings`
rationale assumes the gtk3/atk/gdk bindings are lockfile-only ballast a
Windows build never compiles. On Linux they ARE compiled, so that reasoning
needs revisiting rather than inheriting.
