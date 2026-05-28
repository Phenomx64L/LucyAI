# Changelog

All notable changes to Lucy Assistant are documented in this file.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com),
and this project adheres to [Semantic Versioning](https://semver.org).

---

## [1.4.1] — 2026-05-28

The largest release since 1.4.0. Massive expansion in three directions:
**(1)** new differentiators — Replay deterministic mode, Memory Graph
visual, Session Recording — that no conventional AI tool (Cursor, Cline,
Hermes, OpenInterpreter) currently ships; **(2)** production hardening —
DB backup/restore, support bundle export, retired the broken Skills
manager; **(3)** SRE-grade integrations — Inventory drift detection,
multi-host log timeline, hash chain verification for incident audits,
Dashboard expansion with page file + temperatures + network + failed
logins + process lineage badges. 172 Rust tests + 117 vitest tests +
0 svelte-check warnings + 0 cargo warnings end-to-end.

### Tier S — Non-replicable differentiators

- **Replay Deterministic Mode** (`replay.rs` + `ReplayBrowserView.svelte`).
  Every successful LLM turn is auto-captured into `replay_snapshots` with
  the EXACT prompt + context + system + model + effort. The browser lets
  the operator re-execute any past turn against the same OR a different
  model, with a shingled-Jaccard drift score quantifying how much the
  response moved. No other agent tool captures the full reproducible
  context — chat transcripts are not the same.
- **Memory Graph 2.0** (`memory.rs::memory_graph` + `MemoryGraphView.svelte`).
  Force-directed visualization of `agent_memories` with edges from three
  signals: tag Jaccard, content Jaccard, AND embedding cosine when the
  user has populated the `embeddings` table. Label-propagation community
  detection (Louvain-lite) auto-colors clusters. Runtime threshold
  sliders, top-tag pill filter, search bar, hover-neighbor highlighting,
  anti-collision labels, auto-fit-to-view. Custom 200-line force sim
  instead of D3 (saves ~70 KB).
- **NexShell Session Recording** (`shell_recording.rs` + `ShellRecordingPlayer.svelte`).
  Every cmd/out/err/exit event of a remote shell is time-coded with
  millisecond resolution into SQLite. Playback overlay with timeline
  scrubber, 0.5×/1×/2×/5×/instant speeds, per-event color coding, forward
  coalescing for smooth scrubbing. Asciinema-class capability native to
  the Lucy binary.

### Tier A — Robustness / paridad superada

- **Hierarchical forks + cost ledger** (`fork_results` schema migration).
  `fork_save` accepts `parent_task_id`; `fork_update` accepts `tokens_in`/
  `tokens_out` and computes `cost_usd` server-side via the per-vendor
  pricing table. `ForksMonitorPanel` renders tree structure with indent
  (`└─`) and shows aggregated cost across visible forks.
- **Smart LLM filter in LogViewer**. Free-text "describe what to find"
  input → Gemini Flash Lite translates to regex → applied with substring
  fallback when LLM output is malformed.
- **Capacity projection overlay** (`capacity::capacity_projection`).
  OLS linear regression with R² over 14-day samples; Dashboard pills
  show `↗ 12d to 95%` with tier-colored urgency.
- **CVE matching curated DB** (`cve_match.rs`). 30 high-impact CVEs
  (Log4Shell, EternalBlue, Heartbleed, regreSSHion, XZ backdoor,
  PrintNightmare, Zerologon, etc.) matched against inventory software
  with lenient semver + canonical name aliasing. 5 contract tests.
- **Inventory drift detection** (`inventory_drift.rs` + `inventory_baselines`
  schema). Per-host baseline snapshot; on rescan, computes structured
  diff (software / ports / services / certs / scheduled) with added /
  removed / changed categorization. 6 contract tests, including
  case-insensitive software name match.

### Tier B — Polish & UX

- **Cost-aware automatic routing** (`smart-router.ts`). `economyMode`
  flag tightens heavy-tier promotion gates (>1500 ctx vs default 800,
  keyword needs >400 ctx). `costlierBaseline` produces
  `estimatedSavingsUsd` per turn → Settings shows session savings ledger.
- **Branching conversations** (`bifurcarTabDesde` in +page.svelte).
  Ctrl+B or command palette → deep-clones the active tab up to the last
  Lucy reply into a new tab with lineage badge. Original untouched.
- **Theming JSON system** (`theme-loader.ts`). Custom themes as
  validated JSON (whitelist of 9 vars, strict color regex). Import /
  Export via clipboard, persistence in localStorage, dynamic `<style>`
  tag injection. **16 vitest tests** cover validation paths.
- **Multi-window detach**. Tauri `WebviewWindow` opens an independent
  Lucy instance for dual-monitor workflow. Shares the SQLite DB across
  windows but tabs are per-window.

### Sprint A — Production hardening

- **DB backup & restore** (`db_backup.rs`, **+3 tests**). Uses
  `VACUUM INTO` for atomic snapshots; restore validates source SQLite +
  integrity check + Lucy schema markers (`agent_memories` + `audit_chain`),
  writes a timestamped safety backup of the live DB before clobbering.
  Settings UI shows path + size + total rows.
- **Support bundle export** (`support_bundle.rs`). One-click generates a
  timestamped folder with `manifest.json`, `audit_trail.csv`,
  `recent_incidents.json`, `system_snapshot.json`, `token_usage.csv`,
  `diagnostics.json`. Excludes API keys and full memory content by
  design.
- **Skills Manager retired**. The 1250-line modal that never reached
  production quality is removed from the render tree; the modal handler
  becomes a toast pointing users to Runbooks. SkillPicker and
  SkillBrowserModal remain operational.

### Sprint B — SRE differentiators

- **Multi-host log timeline** (`LogTimelineView.svelte`). Fetches the
  same log path from N hosts in parallel; parses timestamps with 4
  strategies (ISO 8601, syslog, bracket-tagged, Apache CLF); merges by
  millisecond into a single interleaved timeline color-coded per host
  via Okabe-Ito palette. Regex filter, level filter, auto-refresh.
- **Inventory drift detection** — see Tier A above.

### Sprint C — Dashboard polish

- **D14 Editable thresholds per host/metric**. `getThresholds()` +
  `sevVarFor()` centralize threshold lookup; per-metric ⚙ button opens
  a floating editor for warn/crit values; persisted in
  `lucy_thresholds_{host}__{metric}`.
- **D15 Open incidents banner** (`dashboard_open_incidents`). Amber
  banner at the top of the Dashboard with count + most-recent incident
  title + CTA "Open incidents view →". 15-second refresh cadence.
- **D17 Failed logins card** (`dashboard_failed_logins_24h`). Windows
  Security event 4625 in the last 24h. Handles `ACCESS_DENIED` for
  non-admin Lucy gracefully ("Requires admin to read Security log").
- **D18 Process lineage badges** (`dashboard_process_lineage_brief`).
  Top-processes table shows `● new` badge for processes first seen in
  the last 24h via `process_lineage` table integration.
- **D11 Reorderable widgets**. HTML5 drag & drop on dashboard sections
  (CPU cores / storage / processes); per-section hide toggle; reset
  button when layout is customized; persisted in
  `lucy_dashboard_section_order` + `_hidden`.

### Sprint D — Audit & memory pendientes

- **Hash chain verification UI** (`hash_chain.rs`, **+3 tests**).
  Recomputes the SHA-256 chain over `incident_action` rows, compares
  stored vs expected, lists mismatches with position + reason. Surfaced
  in IncidentPanel with green ✓ or red ⚠ badge + per-mismatch detail.
- **Memory health timeline**. 30-day creation histogram above the
  Memorias list, color-tier bars (top tercio green / middle cyan /
  bottom transparent), tooltips per day.

### Sprint E — Crystal viewer redesign

- **Crystal viewer redesigned** (MemoryBrowserView crystals tab).
  Gradient cards with shimmer, structured header (icon + id + project +
  stat pills), border-left-coded sections (outcomes green / files blue /
  lessons amber), file list with `›` bullets, mono footer with session
  preview. Empty state with Diamond icon + onboarding explanation.

### Dashboard expansion (no-Tier sprint)

- **Page file / swap card** when host has swap configured.
- **Temperatures card** when sensors are accessible (CPU/GPU/disk via
  `sysinfo::Components`).
- **Network throughput card** with cumulative MB/s delta tracker
  (`LAST_NET` snapshot Mutex); per-interface badges sorted by traffic.

### Memory module polish

- **M1 Stats dashboard**. Six metric cards at the top of the Memorias
  tab (total, pinned, untagged, expiring <7d, new this week, top
  importance) with hot/warn coloring.
- **M2 Bulk operations**. Per-card checkbox + bulk bar with
  Clear / Select all / Add tag / Promote +1 / Delete actions.
- **M3 AI suggest tags**. Gemini Flash Lite proposes 3-5 tags per
  selected memory; per-memory ✓ Apply / ✕ Reject confirmation panel.
- **Memory consolidation** (`memory_consolidate`). Jaccard clustering
  over recent memories with `superseded_by` marking; preview mode
  default.

### Activity Feed widget

- **24-hour activity rollup sidebar widget** (`activity_feed.rs` +
  `ActivityFeedWidget.svelte`). Aggregates incidents + audit_trail +
  scheduled_runs + state_snapshots + process_lineage + frontier_telemetry
  into a single chronological feed. Moved to the Registros section
  after initial placement caused Memoria/Capacidad/Diagnóstico to
  scroll off-screen.

### Infrastructure

- **Tavily API key UI** in Settings. Reads keyring status (boolean
  only — value never crosses IPC), validates prefix `tvly-` and length,
  enables / clears via password input.
- **DDG search cascade** (`local::search_web`). Three-endpoint fallback
  (`lite.duckduckgo.com/lite/` first, then `html.duckduckgo.com`, then
  `duckduckgo.com/html/?ia=web`). Lite endpoint with stable `<table>`
  parser is the primary fix for the silent-failure mode where DDG
  changes class names.
- **Prompt caching telemetry** (`get_cache_stats`). Process-wide
  Mutex-protected accumulator of Anthropic cache_creation/read tokens
  with hit% computation; surfaced as `⚡ X% caché` footer badge.
- **Test pipeline**: `npm run test:full` runs svelte-check + vitest +
  cargo test in sequence; pre-commit hook tightened (svelte-check
  baseline 0 errors, vitest enforced on frontend commits).
- **Schema migrations**: `inventory_baselines`, `replay_snapshots`,
  `shell_recordings` + `shell_recording_events`, ALTER TABLE on
  `fork_results` for hierarchical + cost columns.

### Bug fixes

- **Sub-Agents panel poll only-when-running**. Polling condition required
  `forks.some(status === 'running')` which was false on empty array →
  panel never refreshed when opened before forks landed. Now polls
  unconditionally every 3s while visible.
- **Streaming → lucy token recompute**. Streaming messages were promoted
  in-place to role 'lucy' without recomputing the token count → tab
  budget undercounted long Lucy responses. Fixed in 3 promotion sites.
- **SSH key pre-flight validation**. NexShell now calls `path_exists`
  before invoking ssh so a typo'd key path gets a precise error instead
  of "Permission denied (publickey)".
- **Cost predictor prefix fallback** (`model-pricing.ts`). Unregistered
  `claude-*` / `gpt-*` / `gemini-*` models used to fall through to
  FALLBACK_PRICING (provider:'openai'); now route to the correct vendor
  tier. Caught by the new `test:full` script.
- **Memory Graph autofit cap + label collision**. Autofit was zooming
  past 1.0× on small graphs, magnifying everything including labels.
  Now caps at 1.0× and counter-scales font-size via `11/zoom`.
  Anti-collision hides labels of lower-degree neighbors within 90px.

### Stats

- **+27 Tauri commands** registered.
- **+15 backend modules** added.
- **+10 frontend components** added (LogTimelineView, MemoryGraphView,
  ReplayBrowserView, ShellRecordingPlayer, ActivityFeedWidget, and
  more).
- **+5 SQLite tables** + extensive ALTER TABLE migrations.
- **172 Rust tests** (vs ~134 in 1.4.0).
- **117 vitest tests** (vs ~32 in 1.4.0).
- **Zero warnings** across svelte-check, cargo, vitest.

---

## [1.4.0] — 2026-05-16

The largest single release since the project started. Driven by a
three-auditor independent code review (security · frontend bugs ·
performance) — every CRITICAL and HIGH finding from that audit is
closed in this version, plus three new integrations and a substantial
UI refresh.

### Security — 5 structural vulnerabilities closed

- **S1 · WinRM password injection** (`utils/shell.rs`). Stored
  passwords no longer interpolate into a PowerShell single-quoted
  literal — the wrapper now `[Console]::In.ReadLine()`s the password
  from stdin and the script body is dispatched via `-EncodedCommand`.
  A stored credential containing `';iex(...)#` is no longer a quote-
  escape attack.
- **S2 · cmd /C blocklist bypass** (`commands/local.rs::execute_cmd`).
  Replaced static substring blocklist (`"format "`, `"del /s"`) with
  the same bypass-token flow PowerShell uses. Patterns like
  `for %i in (...) do %i` and `%COMSPEC% /c` now require an explicit
  user-typed token to proceed.
- **S5 · SSRF via fetch_url + redirect chains** (`commands/ai.rs`,
  `state.rs`). `HTTP_CLIENT` now ships a hop-by-hop redirect policy:
  every `Location:` is validated against `guardrails::scan_url`.
  Loopback, RFC1918, link-local, AWS/Azure/GCP cloud-metadata FQDNs,
  and any non-http(s) scheme are rejected before the body comes back.
- **S6 · Path traversal on Windows** (`commands/local.rs`). New
  centralized `enforce_sensitive_path()` rejects `\\?\` / `\\.\`
  verbatim UNC prefixes, blocks `~/.ssh/`, AWS credentials, Azure
  tokens, Chrome login data, DPAPI master keys, and Lucy's own
  `%APPDATA%\Lucy\` store. Applied symmetrically to read AND write
  (previously read had no blocklist).
- **S10 · UAC elevation injection** (`commands/shell.rs`). Explicit
  patterns — `Start-Process -Verb RunAs`, `.ShellExecute('runas')`,
  `runas /user:administrator|system` — now route through the same
  bypass-token flow as `Remove-Item -Recurse`.

### Security — Defense-in-depth layer

- **New `guardrails/` module** (Rust). Pattern-based scanner with 12
  regex rules drawn from the audit findings + classic prompt-injection
  + hidden Unicode tag (U+E0000..U+E007F) blocker. Role-aware:
  `User` / `Tool` / `Assistant` / `SecretMaterial` get different
  pattern banks. Three-state decision (`Allow` / `HumanInTheLoop` /
  `Block`) maps onto the existing token flow.
- **Wired at 6 call sites**: `fetch_url_content`, `execute_cmd`,
  `execute_powershell`, `read_file_content`, `run_winrm_sync`,
  `spawn_winrm_streaming`.
- **Tauri commands**: `guardrail_scan`, `guardrail_scan_url`,
  `prompt_guard_status` — frontend pre-checks user input before
  sending to the LLM, surfaces a red bubble on Block + native
  confirm on HumanInTheLoop.
- **Footer badge 🛡 GUARD** confirms the layer is active.
- **LlamaFirewall Phase 2 — PromptGuard 2 ONNX (optional)**. Behind
  the `ml-guard` Cargo feature. When the feature is built AND the
  model is installed at `%APPDATA%\Lucy\guardrails\prompt_guard_2\`,
  ambiguous inputs get a second ML pass. Score ≥0.85 promotes to
  Block; 0.5-0.84 promotes to HumanInTheLoop. ML never overrides a
  regex Block. Footer badge 🧠 ML when active. See
  `src-tauri/src/guardrails/PROMPT_GUARD_INSTALL.md`.

### Bug fixes — 4 frontend CRITICALs from the audit

- **F1 · `runAI` race condition** (CRITICAL). Closing a tab mid-stream
  used to leave `_drainTimer` running, mutations going to a phantom
  `t.messages`, and CPU pegged for the duration of the LLM call.
  Now `_runToken[tabId]` is bumped on tab close and `_bailIfStale()`
  is consulted after every await; cleanup of `_drainTimer` and
  `_reasoningTickerRef` happens deterministically.
- **F2 · `getTab()` missing optional chaining**. Three callers
  (`addThinking`, `process`, `cancelpending` event) accessed fields
  on a possibly-null tab; closing a tab between click and execution
  threw TypeErrors that bubbled to `unhandledrejection`. Guards added.
- **F3 · `_scheduledTick` initial setTimeout leak**. The 30s initial
  setTimeout was orphaned, surviving HMR re-mounts. Stored in
  `_scheduledTickInitial` ref and cleared in `onDestroy`.
- **F4 · `checkCancel` interval leak in NexShellView**. The success
  path inside `guardCheck` didn't clear the polling interval —
  it spun until `guardAssessment` happened to be set. Both paths
  now clear deterministically.

### Performance — 3 measurable wins

- **P1 · Stream-reveal throttle**. Old `renderRevealed` ran on every
  drain tick (~33fps) → for a 2k-token response, ~165-330ms of CPU
  re-parsing the growing prefix with `marked` + `DOMPurify`. New
  throttle: skip re-render if `display.length` grew <8 chars AND
  <50ms passed since last paint. Force flush at stream finalize so
  trailing markdown closes cleanly. **~50% CPU reduction** during
  streaming.
- **P5 · `spawn_blocking` for fs reads in async handlers**. Four
  sites in `commands/local.rs` (read_file_content, edit_file,
  analyze_code, search_files walk) were doing `std::fs::read_to_string`
  inside `pub async fn` handlers, stalling the tokio executor.
  Now wrapped via `tokio::task::spawn_blocking` — the LLM stream
  and UI invokes keep responding while Lucy reads.
- **P11 · SQLite r2d2 connection pool**. Replaced single
  `Mutex<Connection>` with `r2d2::Pool<SqliteConnectionManager>`
  (max 8). PRAGMA WAL mode is now finally useful — concurrent
  readers run in parallel, writes block only briefly. PRAGMA
  `foreign_keys=ON` is set on every pooled connection. Hot
  fire-and-forget paths (audit log, embeddings, cost tracking)
  no longer serialize through the same mutex as chat persistence.

### Performance — Bundle size

- **P3 · Tabler icons per-icon imports**. 184 icons across 25 files
  migrated from `import { IconX } from '@tabler/icons-svelte'` to
  `import IconX from '@tabler/icons-svelte/icons/<kebab-name>'`.
  Helper script at `scripts/migrate-tabler-imports.mjs` —
  idempotent, re-runnable.
- **P4 · jspdf lazy import**. `ReportGenerator.ts` switched to
  dynamic `await import('jspdf')` inside each export function.
  Type-only `import type jsPDFType` keeps compile-time signatures.
- **Net bundle delta**: -482 kB raw / -158 kB gzip on the main
  chunk (1,360 kB → 878 kB raw; 461 kB → 303 kB gzip).
  `jspdf` is now a 386 kB dedicated chunk loaded only when the
  user clicks "Export PDF".

### Architecture — Sprint D refactor

- **Per-tab revision stores** (`src/lib/page/tabs-store.ts`).
  `getTabRevStore(id) → Writable<number>` lazily allocates one
  store per tab. `bumpTab(id)` updates ONLY that tab's store +
  global `tabsRev` counter. `ChatThread.svelte` now subscribes to
  its own tab's store via `$revStore` — cousin tabs streaming no
  longer re-render every ChatThread. Token-stream hot path
  (`renderRevealed`) switched to `refreshSoft(tabId)` instead of
  the old `tabs = [...tabs]` cascade.
- **8 modules extracted from `+page.svelte` monolith** (1,400+ LOC
  moved to `src/lib/page/`): `tabs-store.ts`, `workspace-presets.ts`,
  `agent-checkpoints.ts`, `chips-quick-actions.ts`, `fix-store.ts`,
  `host-preflight.ts`, `mcp-secrets.ts`, `ql-popover.ts`. Each
  module: TypeScript strict, exported interfaces, correct
  lifecycle (dispose/detach), independently testable.
- `+page.svelte` LOC: 8,295 → 8,069 (net delta after additions).

### Architecture — MEDIUM polish (5 audit findings)

- **F6 · JSON.parse guards** in three hot paths (`_leerSesiones`,
  `memoria_buscar` tag formatting, `_leerHostsSeguro`) — a single
  corrupt row no longer poisons the whole operation.
- **F7 · `destroyEnrichedWidgets()` on tab close** — Svelte
  components mounted inside messages (process tables, disk bars)
  no longer leak across tab open/close cycles.
- **F9 · `updateScrollState` debounced**. The reactive
  `$: if (tabs.length) setTimeout(updateScrollState, 100)` used to
  spawn dozens of orphan timers per turn during streaming. Now a
  single shared `_scrollStateTimer` ref + `scheduleScrollStateUpdate(ms)`
  helper coalesces bursts.
- **F11 · `auditTrail` localStorage write debounced (200ms)**.
  `persistedWritable` in `stores.ts` now coalesces rapid bursts of
  store updates. A 10-entry audit log burst → 1 disk write, not 10.
  `beforeunload` flushes pending writes.
- **F12 · `metricsHistory` duplicate import removed** from
  `DashboardView.svelte`. The component-local map is the source of
  truth for sparklines; `pushMetricsSample` mirrors to the global
  store for PostureStrip — the unused import that suggested two
  sources of truth is gone.
- **F5 · `console.warn` → `reportSilent()`** at 2 fire-and-forget
  invoke sites — failures now surface in `window._lucyErrors`
  instead of vanishing.

### New integrations

- **Tavily web search backend** for `<TOOL>search_web</TOOL>`.
  When `tavily_api_key` is configured in keyring, search calls go
  through Tavily's agent-optimized API: AI-summarized answer +
  ranked structured results with score, no HTML scraping, no
  aggressive rate limits (free tier: 1000 searches/month).
  Falls back to DuckDuckGo scraping when no key is configured —
  feature works out of the box without any setup. UI in
  ProviderConfigModal → Tavily tab.
- **Mem0-inspired memory patterns** (Rust native, no Python
  dependency). `save_agent_memory` now returns
  `{ id, action: "inserted" | "duplicate", reason }`. Auto-dedup
  via FTS5 bm25 probe: if `score < -8.0` the existing row is
  touched (access_count++) and its id returned instead of a new
  insert. `search_agent_memories` uses a composite ranking:
  `bm25 - 0.5×importance - 0.3×log2(access_count+1) - 2.0×exp(-age/86400)`,
  so recently-accessed memories outrank stale ones. New schema
  columns: `last_accessed_at`, `access_count`, `superseded_by`.
  New Tauri command `supersede_memory(old, new)` for conflict
  resolution.

### UX

- **Inline flag autocomplete** in the chat input. Cursor on a
  `-flag-shaped` token of a known command (rm, find, grep, Get-*)
  triggers a popover with hand-curated flag descriptions.
  Destructive flags (`-rf`, `--force` on rm/rmdir, etc.) are
  always sorted LAST and rendered in red with a ⚠ icon. Tab/Enter
  to insert, ArrowUp/Down to navigate, Esc to dismiss.
  Powered by the same `command-signatures.ts` catalog the
  Guardian uses for safety analysis.
- **Settings → Providers loads configured state on open**.
  Previously every tab showed "not configured" even when a key
  was in the keyring — now `get_configured_providers` is probed
  on mount and re-probed on every modal open. Green checkmarks
  appear immediately for known-good providers.
- **New "🛡 Guardrails" tab in ProviderConfigModal**. Shows the
  regex bank (always-on, green) and the PromptGuard 2 ML status
  (Active / Model missing / Runtime missing / Failed / Feature
  disabled) with re-check button and actionable hints linking to
  the install guide.
- **Footer 🧠 ML badge** showing PromptGuard 2 status — visible
  only when the feature is built + something useful to report.
  Re-probes on window focus so installing the model and alt-tabbing
  back refreshes status without restart.

### Cloud LLMs — Catalog refresh (carried from 1.3.1)

- Anthropic: Opus 4.7, Sonnet 4.6, Haiku 4.5
- Google: Gemini 3.1 Pro, Gemini 3 Flash, Gemini 3.1 Flash-Lite
  (gemini-2.5 family removed from picker; still valid in ALLOWED_MODELS)
- OpenAI: GPT-5.5, GPT-5.5 Instant, GPT-5.4 Mini/Nano, GPT-5.3 Codex
- Distinctive emoji icons per model tier (replaces generic `▾`/`▸`)

### Dependencies

- Added `r2d2` 0.8 + `r2d2_sqlite` 0.24 (connection pool)
- Added (optional, behind `ml-guard` feature): `ort` 2.0.0-rc.10,
  `tokenizers` 0.21, `ndarray` 0.16
- Default build deps unchanged for users not opting into ML guard

### Breaking changes

None for users. **API contract change** for code that calls
`save_agent_memory` directly: return type changed from `i64` to
`SaveMemoryResult { id, action, reason }`. The single internal
caller in `+page.svelte` was updated; external callers (if any)
need to read `.id` instead of the raw return value.

---

## [1.2.1] — 2026-04-28

A focused **stability + observability + visual** release. No breaking
changes; every existing flow keeps working, just feels sharper.

### Added — New capabilities

#### Observability
- **Statistical anomaly detection** on Dashboard CPU / RAM cards.
  A `σ` badge surfaces when a value deviates ≥3σ from the host's
  rolling window (only `strong` ≥3σ and `extreme` ≥4σ; `mild` 2σ
  hidden to avoid alert fatigue).
- **Live cost predictor** in the input bar. Estimates input/output
  tokens and USD before sending; confidence level (low/med/high)
  derived from historical samples for the active model.
- **Notebook export** — turn any chat tab into a portable
  `.lucynote` (JSON envelope, version-stamped) or `.md` runbook
  via the Command Palette. Preserves `user / lucy / thought /
  command / tool` cell semantics for replay.
- **Ambient state indicator** in the footer — a 12 px orb that
  breathes with Lucy's state:
  - idle: soft green pulse 4 s
  - thinking: fast cyan pulse
  - executing: amber arc sweep
  - error: red flash 1.2 s, settles back

#### UX
- **Fuzzy search in Permission Rules** — diacritics-insensitive
  substring matching against pattern + description + applies_to
  + action filter (`all / allow / block / ask`).
- **Stagger reveal** on Audit Trail entries, Multi-host modal,
  ForksMonitor rows. Capped at 360 ms total delay.
- **Hover lift** on Dashboard cards (`-2 px translate + tinted
  shadow`).
- **State-aware input border** — input glow color follows
  Lucy's current state.
- **Mesh gradient ambient** — three subtle radial gradients
  drift at 30 s cycle behind the UI, picking up the state color.

### Changed

- **Motion system unified** — replaced 250+ ad-hoc timings with
  `--motion-instant / fast / base / slow / deliberate` tokens
  + `--ease-out / --ease-spring / --ease-back`. Single source of
  truth for transition cadence.
- **Variable fonts** — Inter Variable + JetBrains Mono Variable
  declared explicitly with proper system fallbacks.
- **Tutorial tour** — refreshed for v1.2.1 with three new
  spotlights (anomaly detection, cost predictor, status indicator)
  and a centered welcome step. The completion flag now stores the
  Lucy version, so future releases automatically re-open the tour.
- **Setup overlay** — version badge + collapsible "What's new in
  v1.2.1" panel.
- `focusTrap` action auto-applies `tabindex="-1"` + `aria-modal=
  "true"` so every dialog using it satisfies a11y requirements
  without per-callsite changes.

### Fixed

- **Multi-step prompt fix** — Lucy used to stop mid-task when
  asked something like *"check my specs **and** search the web
  for tweaks"*. The response parser short-circuited after the
  first tool, killing the agent loop. Now multi-intent prompts
  (sequencing connectors, ≥2 imperative verbs, web+system
  pairing) are detected and Lucy stays in the loop until the
  full task completes.
- **NexShell host cards no longer vanish** when ≥2 sessions are
  active. Caused by a CSS animation race (`opacity:0` + delayed
  entrance + frequent reactive churn re-emitting
  `nsHostsSorted`). Migrated to a Svelte `in:` transition that
  runs only on mount/unmount, never on re-renders.
- **Reasoning bubble timer** advances live (250 ms ticker) even
  when the model emits only tool tags without `<THOUGHT>` chunks.
  Previously stuck at `0.0 s`.
- **Skeleton zombie purge** — empty `streaming` placeholder
  bubbles no longer linger after agent loops that produce no
  streamed text.
- **Status orb position** — moved from a floating element to
  inline-in-footer, no longer overlapping the language code.
- **Tutorial selectors** — fixed case-sensitivity bug on Skills
  step (sidebar was illuminating entirely instead of the Skills
  item) + thin-element padding for the footer step.
- **Blank screen at startup** caused by orphan
  `storedActions / storedChips` references after a refactor.
- **`localStorage.clear()` reduced** to a Lucy-prefixed loop in
  `RESET_APP` to avoid wiping unrelated state.

### Security

- **XSS fix** in code-block rendering — `langLabel` (AI-controlled
  markdown lang string) is now `textContent`-rendered, not
  `innerHTML`. Prevented prompt-injection-driven script execution.
- **6× `.unwrap()` → `map_err`** in MCP serialization. No more
  thread panic on edge-case JSON.
- **Release profile hardening** — `lto = "fat"`, `opt-level = "z"`,
  `codegen-units = 1`, `strip = true`, `panic = "abort"`,
  `incremental = false`. Smaller, denser binary with no symbol
  trail for Ghidra/IDA.
- **String obfuscation (`obfstr`)** on the PowerShell blocklist.
  `strings lucy.exe | grep` no longer reveals the list of dangerous
  patterns Lucy refuses to run.
- **Boot-time integrity check (TOFU)** — SHA-256 of the running
  `.exe` is compared against an anchor at `%APPDATA%\Lucy\.integrity`.
  Logged-only on mismatch; new releases legitimately mismatch.
- **Win32 `IsDebuggerPresent`** check at boot, logged.
- **Vite production hardening** — sourcemaps off, banner comments
  stripped, `console.log / debug / trace` removed in production.
- **`npm audit`**: 5 vulnerabilities (2 high, 2 moderate) → 3 low
  (transitive `cookie` only, not exploitable in a Tauri webview).

### Internal

- **0 warnings** across `svelte-check` (28 → 0) and `cargo check`
  (15 → 0).
- All `localStorage` calls migrated to safe wrappers
  (`safe-ls.ts`) that never throw on quota / corruption.
- `lucy_agent_loop.log` now rotates at 10 MB with 3 historical
  files via a new generic `rotate_log()` helper.
- `_qlPopover` singleton + active streaming AI requests cleaned
  up in `onDestroy` (HMR-induced leaks gone).

### New library modules

| Module | Purpose |
|--------|---------|
| `$lib/anomaly.ts` | z-score statistical anomaly detection |
| `$lib/cost-predictor.ts` | pre-flight token + USD estimation |
| `$lib/notebook.ts` | export/import chat sessions as `.lucynote` |
| `$lib/safe-ls.ts` | localStorage wrappers that never throw |
| `$lib/security.ts` | destructive command pattern detection |
| `$lib/text-utils.ts` | escape/format/normalize helpers |
| `$lib/md-render.ts` | sanitized markdown rendering with LRU cache |
| `$lib/constants.ts` | LANGS, BACKUP_KEYS, ICON_MAP, COST_PER_1K |
| `$lib/debug.ts` | DEV-gated logger with ring buffer |
| `$lib/stagger.ts` | Svelte stagger transitions for list reveals |
| `$lib/StatusOrb.svelte` | ambient state indicator component |
| `src-tauri/utils/integrity.rs` | self-hash & debugger detection |

---

## [1.2.0] — 2026-04

Three sprints of work landed in this release: Lucy became more
**self-correcting**, **retrieval-aware**, and **memory-persistent**.

### Sprint 1 — ReAct Self-Correction
Lucy parses the exit code of every command. On failure she emits
a `<THOUGHT>` block with (a) probable cause and (b) a *different*
next action — never blindly retrying. After two identical failures
she stops and asks for guidance.

### Sprint 2 — Semantic search over skills & memory
- Local embeddings via Ollama (`nomic-embed-text` default).
- Auto-indexing every saved skill + memory.
- Backfill command for existing data.
- Graceful fallback to TF-IDF + web search if Ollama is offline.

### Sprint 3 — Tiered memory (MemGPT-style)
| Tier | Storage | Scope |
|------|---------|-------|
| CORE | `memory_core` | always-on, cross-session |
| WORKING | `memory_working` | current session |
| EPISODIC | `agent_memories` + FTS + embeddings | cross-session, searchable |

### Other in 1.2.0
- Multi-agent sub-agent picker (Plans A & C) with verifier.
- PDF Intelligence — ingest manuals & semantic RAG.
- Fork persistence + Sub-Agent Monitor UI.
- NVIDIA NIM integration.
- Full security audit pass (XSS, path traversal, key exposure).

---

## Earlier versions

See `git log` for full history before 1.2.0.
