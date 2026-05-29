# Changelog

All notable changes to Lucy Assistant are documented in this file.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com),
and this project adheres to [Semantic Versioning](https://semver.org).

---

## [1.4.8] — 2026-05-29

Follow-up hotfix to v1.4.7. **0 warnings.**

### Fix — "Phase 1 = write a script" misinterpretation

User-reported: after v1.4.7, the Caso 2 audit prompt ran 4 chapter
steps (Locate msedge.exe → powershell → Write collect_compliance.ps1
→ Lectura), then ended with the renderer's generic auto-summary
("Modifiqué 1 archivo … 3 operaciones de lectura/análisis · ✓
Operaciones completadas") instead of the PDF + 5-bullet summary the
user explicitly asked for. Lucy wrote the collection script and
walked away — she never executed it, never built the HTML, never
ran Edge to produce the PDF.

**Root cause**: v1.4.7's Rule 2(c) said "split into phases". The LLM
interpreted "Phase 1" as "write the data-collection script", and
"Phase 2" as "the user runs everything". That's wrong:
collect_compliance.ps1 has NO admin-only operations — Lucy could
have run it herself. The Phase-2 carve-out applies ONLY to the
truly admin-required slice (Get-WinEvent Security 4625), not to
the whole pipeline.

**Fix** (`prompt_sections.rs`):

  - **Tightened Rule 2(c)**: explicitly says "Phase 1 is NOT 'write
    a script and stop'. If you wrote collect.ps1 and it doesn't need
    admin, RUN IT NOW with EXECUTE_CMD, read the output, then BUILD
    THE PARTIAL DELIVERABLE (HTML/PDF/markdown table) in the same
    turn or the next iteration."
  - **Tightened Rule 2(d)**: "If the user asked for a PDF, the PDF
    must EXIST on disk by the end of your turn (with whatever data
    phase 1 produced — even 5 of 7 sections is real progress). 'I
    wrote a collection script' is NOT delivery."
  - **NEW Rule 2b — COMPLETION CONTRACT**: "When the user EXPLICITLY
    asks for one or more deliverables (PDF, CSV, JSON, file, table,
    list, summary of N bullets, dashboard, etc.), your conversation
    MUST end with each deliverable VISIBLE to the user — either
    rendered in the chat or written to disk with the path stated in
    your final narrative. You MUST attempt every deliverable; you
    may not silently drop one. NEVER end a multi-deliverable task
    with only a script written to %TEMP% and no execution — that's
    failing the user, not 'splitting phases'."

Expected on the Caso 2 prompt: Lucy now (1) writes collect.ps1,
(2) EXECUTES it, (3) reads its JSON output, (4) builds the HTML
report, (5) runs Edge headless to produce the PDF on disk, (6)
writes the 5-bullet executive summary in chat referencing the PDF
path. Only the failed-logins (Event 4625) admin section gets a
copy-paste block at the end.

218 Rust tests · 145 vitest · 0 svelte-check warnings.

---

## [1.4.7] — 2026-05-29

Single-rule hotfix for a recurring Caso 2 failure mode. **0 warnings.**

### Fix — Lucy now anticipates her own elevation guardrail

User-reported: on the audit prompt asking for `Get-WinEvent Security`
(failed logins 4625), Lucy wrote a `generate_ciso_report.ps1` script
and tried to execute it with `Start-Process powershell -ArgumentList
"…" -Verb RunAs`. Lucy's own UAC-elevation guardrail correctly blocked
the command (`SECURITY_BLOCK:<token>:-verb runas`) — but the result
was the user not receiving the report at all, because Lucy didn't
anticipate the block and had nothing else to fall back to.

**Root cause**: Rule 2 in SafetyRulesSection only said "DO NOT auto-
generate Start-Process RunAs … ask for confirmation". It didn't tell
Lucy WHICH operations need admin, didn't warn that her elevation will
be silently blocked, and didn't give her a recoverable strategy.

**Fix** (`prompt_sections.rs`): rewrote Rule 2 with four operational
subsections:

  (a) **Recognize admin-only operations** explicitly — Get-WinEvent on
      Security channel, wevtutil cl Security, Set/New-Service,
      sc.exe create, HKLM write, %ProgramFiles%/%SystemRoot% write,
      Get-Process -IncludeUserName, NetSecurity. Also lists what's
      NOT admin: Get-Hotfix, Get-Service, Get-NetTCPConnection, HKCU.

  (b) **Never emit `-Verb RunAs`** — it WILL be blocked, no UAC prompt
      surfaces because Lucy isn't running interactively.

  (c) **Split compound tasks into two phases** for audits asking for
      both admin and non-admin data:
      - Phase 1 (automatic): collect every non-admin section.
      - Phase 2 (user-driven): emit ONE markdown ```powershell```
        block with the admin command, prefixed by 'Para completar el
        reporte, abre PowerShell como Administrador y pega esto:'.

  (d) **Always deliver a partial report** in phase 1 — never withhold
      what Lucy can produce. A report with 5 of 7 sections + a clear
      admin command for the rest beats no report at all.

Expected outcome on the Caso 2 audit prompt: Lucy now collects 5+ of
7 sections (parches, software, puertos, servicios, sin admin) on the
first try without hitting the guard, and emits the failed-logins
command as a final copy-paste block for the user to run elevated.

218 Rust tests · 145 vitest · 0 warnings.

---

## [1.4.6] — 2026-05-29

Two small productivity wins from the Caso 2 benchmark experience.
**0 svelte-check warnings · 0 cargo warnings · 145 vitest tests (+11).**

### Smart routing — better complexity detection

- **Spanish + Portuguese heavy keywords** added to the analysis-intent
  detector (`smart-router.ts`). Previously the HEAVY_RE list was mostly
  English: "auditoría" / "auditoria" / "cumplimiento" / "vulnerabilidad" /
  "diagnosticar" / "explica por qué" / "informe ejecutivo" / "reporte
  para CISO" now route to Claude Sonnet automatically when smart-routing
  is on. Fixes the silent gap where Spanish prompts hitting the same
  semantic content stayed on Flash.
- **Subtask density heuristic** — `subtaskCount(prompt)`. Counts comma-
  /and-separated noun phrases containing SysAdmin nouns (patches,
  software, ports, services, users, report, PDF, …). ≥4 distinct
  subtasks → heavy tier (≥5 in economy mode). Catches structurally
  complex prompts that don't happen to use a heavy keyword.
- **`detectHeavyPrompt` exported helper** — used by the new UI nudge.
  Returns a short reason string when a prompt is structurally heavy.

### Heavy-prompt nudge — UI surface

- **Inline nudge above the input** when the user is typing a heavy
  prompt + has a fast model selected + smart-routing is OFF. Violet
  banner says *"Prompt complejo detectado · 5 subtareas detectadas"*
  with an `Upgrade →` button that swaps the tab to `claude-sonnet-4-6`.
  Dismissible. Wired via the new `upgrademodel` event on ChatInput.
  Fixes the recurring scenario where users hit Send on a multi-task
  audit prompt with Flash and lose 90 s + $0.05 on a truncated result.

### MCP inline enable/disable toggle

- **iOS-style toggle on each registered MCP server** in the modal list.
  Lets the user silence a noisy MCP catalog (e.g. github with 26 tools
  cached) from the system prompt for a specific task without deleting
  the server. Persisted via `mcp_server_upsert` (which already evicts
  pool sessions on update). Optimistic UI — UI flips first, rolls back
  on backend failure. Saves ~2-3 KB of system-prompt budget when only
  one of several servers is needed.

### Tests

- 11 new vitest tests (4 multi-language routing, 2 subtask density,
  5 `detectHeavyPrompt` UI helper).
- 134 → 145 total.

---

## [1.4.5] — 2026-05-29

A stabilization release — four targeted fixes for issues surfaced during
internal benchmark runs. No new features; the goal is to make v1.4.4
boringly reliable. **218 Rust tests · 134 vitest · 0 warnings.**

### Fixes

- **`Respuesta vacía del modelo` false positive on `EXECUTE_CMD`-only
  responses** (commit `fa2a2a1`). The agent-loop entry check at
  `+page.svelte:3931` previously required `FILE_TOOL_RE`, `NATIVE_TOOL_RE`,
  or `<THOUGHT>` to enter the loop. When the LLM emitted ONLY
  `<EXECUTE_CMD>` blocks (no THOUGHT wrapper, common on audit prompts),
  none matched → the response fell through to the empty-response
  detector, which stripped the EXECUTE_CMD block and incorrectly
  surfaced "Safety filter blocked the output" or "Mode collapse". The
  retry usually worked because the LLM non-deterministically wrapped
  with `<THOUGHT>` on the second pass — that's why the user saw it as
  "random". Fix is defense-in-depth: extend the entry condition to
  include `<EXECUTE_CMD>`, `<EXECUTE>`, `<PLAN>`, AND make the empty-
  response detector suppress the warning whenever ANY actionable block
  was present in the raw response. True empty responses (safety filter,
  timeout) still surface correctly.

- **Efficiency regression on long audit/compliance prompts** (commit
  `f8078e6`). Three combined changes addressing the multi-factor root
  cause:
  - **Compact MCP catalog** in the system prompt (`prompt_sections.rs`).
    The v1.4.2 inputSchema injection had grown the catalog to ~4-5 KB
    for users with GitHub MCP (26 tools) + filesystem (11 tools). On
    Gemini Flash with finite output budget for long agent loops, that
    pressure showed up as truncated final commands ("Informe_Seguridad_Com"
    instead of "...Compliance.pdf"). Reduced to 10 tools/server with
    signature only (was 20 with description). Lucy still has the full
    catalog cached locally; she can re-discover for details. Estimated
    saving: 2-3 KB per system prompt.
  - **Stricter writefile loop guard** (`+page.svelte:5837`). The
    generic `checkToolLoop` blocked only on the 4th identical call.
    For writefile-to-the-same-path that's too lenient — once a generated
    script has 2 PowerShell parse errors in a row, the rewrites won't
    converge. Per-tab counter now blocks on the 3rd attempt with an
    explicit hint naming the three recurring failure modes (unbalanced
    `@{}`, missing Catch, broken string interpolation) and telling Lucy
    to SPLIT, not patch.
  - **PowerShell parse-error guard** (`+page.svelte:6304`). New
    detector for parse-error signatures in tool results — "El literal
    de hash estaba incompleto", "Token … inesperado", "Falta un bloque
    Catch/Finally", plus English equivalents. On ≥2 parse errors per
    iteration, injects a strong split-into-smaller-scripts hint into
    the next LLM turn's context. Logged via
    `logTaskEvent('agent_loop_block', 'ps_parse_errors', ...)`.

- **DB integrity false-positive in Self-Diagnostics** (commit `aca96d5`).
  `PRAGMA quick_check` was failing intermittently with "unable to
  validate the inverted index for FTS5 table main.file_index: database
  is locked" when smart_chips, agent_memories, and audit_trail had
  concurrent writes in flight. That's lock contention, NOT corruption.
  Three changes to `check_database_health()`:
  - `busy_timeout(5000ms)` on the diagnostics connection so quick_check
    waits up to 5 s for the conflicting writer.
  - One-shot retry after 200 ms if the result string matches lock
    contention signatures.
  - Triage in the UI message: locked-out result → yellow WARNING with
    "Integrity check skipped (DB busy — transient lock, no corruption).
    Re-run when idle to verify." instead of red ERROR. Real corruption
    still surfaces in red.

- **Simplify-skill cleanup pass on the v1.4.4 diff** (commit `ce6634e`).
  4-agent code review (reuse / simplification / efficiency / altitude)
  applied 5 fixes plus the LiveTraceDock removal users had asked for:
  - **DELETE `file_diff.rs`** (164 LOC + 5 tests). Dead code: frontend
    used the existing JS line-by-line renderer in `renderSingleCardHtml`,
    never called `compute_text_diff` Tauri command.
  - **Fix `cite-chips` digit-corruption bug**. The placeholder/stash
    scheme used bare digit indices ("0","1",…) and restored via
    `/(\d+)/g`, silently clobbering user-prose numbers ("port 8080",
    "error 42"). Switched to non-collidable `\x01`-sentinel
    placeholders. Added a regression test asserting no sentinels leak
    to output.
  - **Extract `mdCell()` helper** in `notebook.ts`. user/lucy/thought/
    tool branches built the same markdown cell shape via copy-paste.
    `mdCell(header, body)` + `splitPreservingTrail` shrink
    `notebookToIpynb` from ~70 to ~45 LOC.
  - **Move `window._lucyWriteUndo` → `t._writeUndo`**. Global Map
    collided across tabs and leaked across reloads. Per-tab scope.
  - **Replace pause spin-wait with event-driven Promise**.
    `while(t._paused) await setTimeout(200)` was a polling loop.
    `togglepause`-off and hard cancel drain `t._resumeWaiters[]`
    immediately. Zero polling, sub-frame resume latency.
  - **REMOVED `LiveTraceDock.svelte`** (-245 LOC). User flagged the
    always-visible vertical sparkline as redundant with the existing
    FAB. Deletion restores the original FAB-only UX.

### Numbers

- **218 Rust tests** (same — 5 file_diff tests removed, 0 net change
  in test surface for fixed code paths) — all green.
- **134 vitest** (+1 regression test for cite-chips digit corruption) — all green.
- **0 svelte-check warnings**, **0 cargo warnings**.
- Net LOC: −420 (file_diff removal + dock removal + simplifications)
  offset by ~80 LOC of new guard code.

---

## [1.4.4] — 2026-05-29

The "finish what we started" release. Closes most of the remaining Terminal
IA quick wins (E inline diff in tool cards, F granular cancel, H inline
cite-chips), ships MVPs of both Moats (J replay button, K notebook export),
and cleans up MCP regression testing + README documentation. **223 Rust
tests + 3 MCP integration tests · 133 vitest · 0 warnings.**

### Terminal IA

- **Inline cite-chips** (`cite-chips.ts`, 16 vitest tests). Lucy's prose
  is post-processed after Markdown sanitization to wrap recognized
  entities in clickable spans:
  - File paths (`C:\…\foo.txt`, `/var/log/x.log`, `./src/foo.ts`) →
    open in VSCode
  - Memory IDs (`memoria #42`, `memory #7`, `[mem:128]`) → switch to
    Memory Browser and highlight the entry
  - Hosts (`@server-prod`) → drop `@<host>` into the input so the next
    prompt is scoped to that host
  - URLs (`https://…`) → open in OS default browser via PowerShell
  
  Idempotent (re-running on already-chipped HTML is a no-op), case-
  insensitive, skip-zone-aware (never wraps inside `<code>`, `<pre>`,
  `<a>`, or existing chips). Color-coded by kind (green file / violet
  memory / blue host / amber URL) with subtle baseline and stronger
  hover/focus states.
- **Granular cancel** — three buttons replace the single Stop:
  - ⏸ **Pause** — agent loop spin-waits between iterations until the
    user resumes or hard-cancels. 200ms poll.
  - ⏭ **Skip next tool** — synthesizes a "user skipped" stub so the
    agent continues without that tool's output.
  - 🛑 **Stop** — the existing kill-everything path. Granular flags are
    reset on hard cancel so the next turn starts clean.
- **Inline diff in writefile tool cards**. The agent loop now reads the
  OLD content before writing, then attaches `{oldStr, newStr}` to the
  tool card. The existing `renderSingleCardHtml` diff renderer paints a
  line-by-line side-by-side view (`.tc-d-ad` / `.tc-d-rm` / `.tc-d-eq`
  classes). Per-file undo buffer is captured so `/revert <path>`
  restores the pre-write content with one command.

### Moats

- **Replay button per Lucy turn** (`⏪`). Sits in the message-bubble
  action cluster (left of Branch ⌥, left of Pin ·). Opens the existing
  ReplayBrowserView pre-scoped to that snapshot. Cyan accent
  distinguishes it from the amber pin and teal branch.
- **Notebook export to `.ipynb`** (`notebookToIpynb()` in `notebook.ts`).
  New `/notebook` slash command — builds a Lucy Notebook from the tab
  via the existing `buildNotebook()`, converts to nbformat-4 JSON,
  picks a save path via `rfd::FileDialog`. Cells:
  - `user` / `lucy` / `thought` → markdown cells with role headers
  - `command` → code cells with language hint (`powershell` /
    `shell` / `bash`) and the captured output as `stream:stdout`
  - `tool` → markdown cells with fenced output blocks
  
  The notebook metadata includes `lucy.source_version`, `lucy.model`,
  `lucy.created_at`, and `lucy.lang` so downstream tools (or future
  Lucy versions) can re-import it.

### Cleanup

- **MCP regression test with mock JSON-RPC server** (`src-tauri/tests/
  mcp_mock_server.py` + 3 new `#[ignore]` tokio tests). The Python
  mock implements `initialize` / `tools/list` / `tools/call`, supports
  optional response delay (`MCP_MOCK_SLEEP_MS`) and forced-failure mode
  (`MCP_MOCK_FAIL=1`). Tests exercise the full spawn → handshake →
  call → close lifecycle AND prove the pool reuses sessions (call
  counter goes 1 → 2 → 3 across three sequential `pooled_call`s).
  Marked `#[ignore]` so they run on demand via
  `cargo test -- --ignored` (needs Python on PATH).
- **`compute_text_diff` Tauri command** + `file_diff.rs` (5 tests).
  Backs the inline-diff tool card. Unified-diff format via the `similar`
  crate, capped at 200 lines with a `[truncated]` footer. Returns
  `{text, additions, deletions, truncated}`.
- **README MCP section** — quick-start (filesystem, no key) and
  GitHub-with-token walk-throughs, curated preset table, architecture
  highlights, diagnostic commands.

### Numbers

- **223 Rust tests** (+5 net new: 5 file_diff) + **3 MCP integration
  tests** (run via `--ignored`) — all green.
- **133 vitest** (+16 net new: cite-chips) — all green.
- **0 svelte-check warnings**, **0 cargo warnings**.
- Net LOC: ~1,500 (backend ~700 / frontend ~800).

---

## [1.4.3] — 2026-05-29

A polish release. Focused on Terminal IA improvements (auto-titled tabs,
pinned-messages strip), the always-visible live-trace dock, and cleanup
chores (chip telemetry exposed via `/chip-stats`, flaky shell tests
stabilized). **218 Rust tests · 117 vitest · 0 warnings.**

### Terminal IA

- **Auto-titled tabs** (`generate_tab_title`). After Lucy's first
  meaningful response, a tiny background call to Gemini Flash (Anthropic
  Haiku fallback) generates a 3-5 word title summarizing the
  conversation. Replaces the previous "first 30 chars of prompt"
  heuristic with something scannable. Skipped under Privacy Mode and
  permanently disabled once the user manually renames the tab
  (`t._titleAuto = false`). Title is sanitized: quotes / fences /
  "Title:" prefix stripped, capped at 5 words / 48 chars. 7 unit tests
  cover the cleaner.
- **Pinned-messages strip** at the top of the chat. Up to 3 pinned
  messages render as horizontally-scrollable chips with a 80-char
  preview. Click → smooth-scrolls to the message with a 1.2s amber
  pulse-highlight so you can spot where you landed. × on the chip
  unpins. Solves "I keep losing the goal in long investigations".

### Moats

- **Live-trace dock** (`LiveTraceDock.svelte`). A 22px vertical
  sparkline anchored to the right edge of the chat area, ALWAYS visible
  when the chat view is open. 60 buckets × 1s = last 60 seconds of
  agent activity, color-coded by dominant phase (violet thought / blue
  llm.turn / amber tool / green exec / red error). Heartbeat dot at
  the top pulses when an event landed in the current second. Click
  anywhere → opens the full LiveTracePanel. Gives operators a
  permanent "is Lucy alive?" signal without opening anything. Mounted
  only on the active tab to avoid per-tab reactive churn.

### Cleanup

- **`/chip-stats` slash command** + `chip_stats_summary` Tauri command.
  Surfaces 7-day rolled-up engagement from `chip_click_log`: total
  clicks/dismisses, unique labels, top 12 by net score (clicks - 0.6 ×
  dismisses). Net score color-coded (green ≥3 / red ≤0). Lets you see
  which suggestions YOU actually use vs the noise floor.
- **Flaky shell tests stabilized**. `powershell_timeout_fires` elapsed
  bound 10s → 20s + timeout 2s → 3s; `stderr_noise_is_filtered` timeout
  15s → 30s. Both contracts unchanged — just give PowerShell cold-start
  (which can take 12-18s on PS-7 first launch) enough slack.

### Numbers

- **218 Rust tests** (+8 net new: 7 title sanitizer, 1 chip-stats math) — all green.
- **117 vitest** — all green.
- **0 svelte-check warnings**, **0 cargo warnings**.

---

## [1.4.2] — 2026-05-29

A focused follow-up to 1.4.1. Three themes: **(1)** first-class MCP
integration matching Claude Desktop / Cursor / Cline; **(2)** smart
chips with real conversational reasoning, not pattern matching;
**(3)** UX polish — settings tabs, richer tab headers, branch-from-here.
**210 Rust tests** (+38 net new) · **117 vitest** · **0 svelte-check warnings**.

### MCP Model Context Protocol — first-class registry

- **`mcp_servers` SQLite table** persists registered servers (name, command,
  env_keys list, tools cache, status, last latency). UX parity with
  `claude_desktop_config.json` — register a server by name once and Lucy
  invokes it as `mcp_query:<name>|||<tool>|||<args>` with the backend
  resolving the command and filtering env keys from Windows Credential
  Manager automatically.
- **6 new Tauri commands**: `mcp_server_list / upsert / delete /
  discover / test / mcp_server_call`. Discover caches `tools/list`
  results; subsequent system-prompt builds list available tools without
  re-spawning the subprocess.
- **`McpServersModal.svelte`** (~520 LOC) — status pills (ok/error/pending),
  cached-tool browser, **per-tool invoke panel** with a JSON args editor,
  6 curated presets (filesystem, github, brave-search, postgres, puppeteer,
  slack), env-key multi-select bound to the existing Keyring bag with
  visible chips (green = present, red = missing).
- **Dual-resolution agent loop**: when the model emits `mcp_query:<arg>`,
  `<arg>` is first checked against the registered server names. If
  matched, the registry path is used (named server, filtered env);
  otherwise the legacy raw-command path runs for backward compatibility
  with prior prompts and skills.
- **`McpRegistrySection`** in the system prompt lists active servers
  with name + command + cached tools. **`inputSchema` injection** —
  each tool now renders with a compact signature like
  `search_repositories(query*: string, sort?: "stars"|"forks"|"updated",
  perPage?: number)`. Eliminated the class of "Validation Failed" errors
  caused by the LLM guessing argument shapes (e.g. `q:me` on GitHub
  Search). 9 unit tests cover enum inlining, ellipsis, nullable arrays,
  union keywords.
- **Connection pooling** (`mcp.rs::pooled_call`). Each MCP `tools/call`
  used to spawn `npx` cold-start (200-800ms) + JSON-RPC init handshake
  + tools/call + kill. **Now the subprocess stays alive between calls**;
  only the tools/call latency is paid (~50-200ms). Observed: 15 sequential
  GitHub MCP calls went from ~45s overhead to ~750ms total. Session keyed
  by `(server_name, command, env_hash)` with a 60s idle TTL and a
  background reaper that wakes every 30s. On any I/O error the session is
  evicted and the next call respawns fresh. `mcp_pool_stats` and
  `mcp_pool_clear` exposed for diagnostics / forced refresh. 5 tests
  cover key stability across env iteration order, secret isolation,
  server/command differentiation.

### Smart chips — three layers of reasoning

- **Layer 1 — LLM-generated chips** (`smart_chips.rs`, 9 tests). After
  each Lucy response, a tiny background call to Gemini Flash (fallback
  Anthropic Haiku) reads the last 6 turns and proposes 1-3 next-action
  chips with structured JSON output (`response_mime_type: application/json`).
  Cost: ~$0.0003/turn. Latency: 400-800ms. The robust parser tolerates
  markdown fences, leading prose, malformed JSON via bracket-counted
  `extract_first_json_array`. Skipped when privacy mode is on.
- **Layer 2 — Heuristic chips** (existing `predictive-chips.ts`). Now
  carries a `source: 'heuristic'` provenance tag.
- **Layer 3 — Memory chips** (`chip_memory.rs`, 9 tests + new
  `chip_click_log` table). Every chip click AND dismiss is persisted
  with its context signature (domains, tool_labels, had_error, lang).
  At chip-generation time, past events with overlapping signatures are
  scored by `clicks × overlap_bonus × recency_decay` (30-day half-life)
  minus 0.6× dismisses. Filtered above a 0.5 floor, top 2 surface with
  the ◊ memory badge — Lucy literally learns which suggestions YOU
  click in which contexts. Pure local SQLite, runs even in privacy mode.
- **`PredictiveChipStrip.svelte`** renders a provenance badge per chip
  (⚡ heuristic / ✦ LLM / ◊ memory) with hover tooltip explaining origin.
  Parent receives `chipdismiss` events alongside `chipaction` so Layer 3
  trains on negative signal too.
- **Parallel orchestration** in `recomputePredictiveChips`: heuristics
  render instantly (<1ms), Layer 1 + Layer 3 fire in `Promise.all`,
  results merged with priority memory > LLM > heuristic via
  `mergeChips()` (4-char substring dedup catches paraphrases like
  "Continue" vs "Continue investigation"). Staleness guard via
  message-id snapshot — late responses for stale turns are discarded.

### UX & polish

- **Settings modal redesigned with tabs** (`Apariencia` / `IA` / `MCP` /
  `Sistema`). Modal widened 420 → 640px, sections rendered as cards with
  rounded borders, two-column grid rows (label fixed-width, control
  flexible) — no more controls jammed to the right. Tabs use **Tabler
  icons** (Palette, Brain, Plug-Connected, Settings) matching the
  sidebar's visual language. MCP tab shows a live badge with the count
  of registered servers.
- **Thinking skeleton bar** rebuilt — 11px dark grey → 14px with
  theme-aware accent-tinted shimmer using `color-mix(in srgb, var(--acc)
  32%, var(--bg4))`. Visible across all themes including
  `custom-neon-tokyo` where the old bar disappeared.
- **Empty-input shortcut overlay**: when the textarea is empty, an
  unobtrusive ribbon renders `Ctrl+P palette · Tab autocomplete · /
  commands · @ host · Esc cancel`. Auto-hides on focus or typing.
- **Brief Mode toggle** (`.brief-btn` in the input bar). When ON,
  prepends `[Modo conciso: responde en máx. 3 líneas, sin preámbulos]`
  to the LLM-bound prompt only — history, tab titles and replay show
  the user's original text unmodified. Persisted in `lucy_brief_mode`.
- **Branch-from-here** button (`⌥`) on every Lucy message bubble.
  Clones the conversation up to that message into a new tab via
  `bifurcarTabDesde`. Previously only reachable via Ctrl+B.
- **Rich tab header**: status dot color-coded by tab activity
  (`processing` blue pulsing / `fork` amber / `error` red / `stale`
  desaturated after 30 min idle / `idle` purple default). 3-letter
  model shorthand pill on the active tab (`gem` / `son` / `hai` / `gpt`
  / `oll` / `nvi`). Hover preview popover enriched with model + turn
  count + token total + accrued cost + state pills.
- **Tutorial expanded** with a new Configuration section covering each
  sub-module (Providers, Privacy/Smart-Router/Economy, Themes JSON,
  Data backup/restore/support bundle, MCP usage, Verifier, Profiles).
  Welcome menu lists v1.4.1+v1.4.2 highlights and explains MCP usage
  step-by-step. Skills Manager step removed (module retired in 1.4.1).

### Numbers

- **210 Rust tests** (+38 new: 9 smart_chips, 9 chip_memory, 9
  signature parsing, 11 mcp pool + base) — all green.
- **117 vitest** — all green.
- **0 svelte-check warnings**, **0 cargo warnings**.
- New SQLite tables: `mcp_servers`, `chip_click_log`.
- Net LOC added: ~2,400 (backend ~1,200 / frontend ~1,200).

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
