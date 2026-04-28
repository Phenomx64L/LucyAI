# Changelog

All notable changes to Lucy Assistant are documented in this file.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com),
and this project adheres to [Semantic Versioning](https://semver.org).

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
