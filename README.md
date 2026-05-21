<p align="center">
  <img src="icon.png" alt="Lucy" width="120" />
</p>

<h1 align="center">Lucy Assistant</h1>

<p align="center">
  <strong>Autonomous SysAdmin AI Assistant</strong><br>
  Desktop application for infrastructure management, compliance auditing, and remote administration — powered by LLM intelligence.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-1.4.0-7dd3fc" alt="v1.4.0" />
  <img src="https://img.shields.io/badge/Tauri-2.0-blue?logo=tauri" alt="Tauri 2.0" />
  <img src="https://img.shields.io/badge/Svelte-5-orange?logo=svelte" alt="Svelte 5" />
  <img src="https://img.shields.io/badge/Rust-2021-brown?logo=rust" alt="Rust 2021" />
  <img src="https://img.shields.io/badge/license-GPLv3-green" alt="GPLv3 License" />
</p>

---

## 👤 Author & Maintainer

**Iván Eduardo Luna** (@Phenomx64L)
- 🔗 [LinkedIn](https://linkedin.com/in/phenomx64l)
- 🐙 [GitHub](https://github.com/Phenomx64L)
- 💼 SysAdmin + Full-Stack Developer

*Lucy was conceived and built as a response to real-world infrastructure administration challenges. Every architectural decision reflects 10+ years of SysAdmin experience.*

---

## Overview

Lucy is a desktop AI assistant designed for system administrators. It combines a conversational LLM interface with real infrastructure tooling — remote shell execution, log analysis, CIS compliance scanning, and credential management — all from a single, secure desktop app.

Built with **Tauri 2** (Rust backend) and **SvelteKit 5** (frontend), Lucy runs natively on Windows with minimal resource overhead.

## What's New in v1.2.1

A focused **stability + observability + visual refinement** release. No breaking changes — every existing flow keeps working, just feels sharper.

### 🛡️ Reliability
- **Multi-step prompt fix** — Lucy used to stop mid-task when given prompts like *"check my specs **and** search the web for tweaks"*. A premature short-circuit in the response parser killed the agent loop after the first tool. Now Lucy detects multi-intent prompts (sequencing connectors, ≥2 imperative verbs, web+system pairing) and stays in the agent loop until the full task is done.
- **NexShell host cards no longer vanish** — connecting to multiple hosts could leave the *Configured Hosts* panel empty while the counter still said `3`. Caused by a CSS animation race (`opacity:0` + delayed entrance + frequent reactive churn). Migrated to a Svelte `in:` transition that runs only on mount.
- **"Thinking…" timer ticks again** — the reasoning bubble showed `0.0s` frozen when the model emitted only tool tags. Added an independent 250ms ticker, hoisted to a runAI-scoped ref so cancellations / errors clean it up too.
- **Skeleton zombie purge** — empty `streaming` placeholder bubbles no longer linger after the agent loop ends.

### 🔍 Observability
- **Statistical anomaly detection** on Dashboard CPU / RAM cards — a discrete `σ` badge surfaces when a value deviates ≥3σ from the host's recent rolling window. Pure stats, no ML, opt-out via `prefers-reduced-motion`.
- **Live cost predictor** in the input bar — estimates tokens & USD before you press Enter, with confidence levels (low/med/high) based on historical samples per model.
- **Notebook export** — turn any chat tab into a portable `.lucynote` (JSON envelope) or `.md` runbook via the Command Palette. Cells preserve `user / lucy / thought / command / tool` semantics for replay.
- **Fuzzy search in Permission Rules** — diacritics-insensitive substring + action filter (allow/block/ask) over patterns and descriptions.

### 🎨 Visual identity (Tier 1: Cursor-aesthetic foundation)
- **Unified motion tokens** — replaced 250+ ad-hoc timings with `--motion-instant/fast/base/slow/deliberate` + `--ease-out / --ease-spring / --ease-back`. Single tactile identity across every transition.
- **Ambient state indicator** in the footer — a 12px orb that breathes with Lucy's state (idle = soft green pulse, thinking = fast cyan, executing = amber sweep, error = red flash). Inspired by Cursor + Linear status dots.
- **State-aware input border** — the input glow + ring color follow Lucy's current state, so the user always knows whether they're waiting or free to type.
- **Mesh gradient ambient** — three subtle radial gradients drift at 30s cycle behind the UI; picks up the state color so the whole window tints with Lucy's mood (clamped to 0.85 opacity, 0.4 on light themes).
- **Stagger reveal** on Audit Trail / Multi-host modal / ForksMonitor lists.
- **Hover lift + glow** on Dashboard cards.
- **Variable fonts** — Inter Variable + JetBrains Mono Variable explicitly declared.
- **Versioned tutorial** — onboarding tour re-opens automatically when the build version changes, with new spotlights for the v1.2.1 features.

### 🔒 Hardening (free anti-tampering layers)
- **Release profile**: `lto="fat"`, `opt-level="z"`, `codegen-units=1`, `strip=true`, `panic="abort"`. Smaller, denser binary; no symbol trail for Ghidra/IDA.
- **String obfuscation** (`obfstr`) on the PowerShell blocklist — `strings lucy.exe | grep` no longer reveals the list of dangerous patterns Lucy refuses to run.
- **Boot-time integrity check** (TOFU) — SHA-256 of the running `.exe` compared against `%APPDATA%\Lucy\.integrity`. Logged-only by design (a fresh release legitimately mismatches the anchor).
- **Win32 `IsDebuggerPresent`** check at boot.
- **Vite production hardening** — sourcemaps off, console.log/debug/trace stripped, banner comments removed.

### ♿ Accessibility & quality
- **0 warnings** across `svelte-check` + `cargo check` (down from 28 + 15).
- **0 unhandled `localStorage` calls** across the codebase (all migrated to `safe-ls` wrappers that never throw on quota / corruption).
- `focusTrap` action now auto-applies `tabindex="-1"` + `aria-modal="true"` to every dialog using it — fixed ~10 dialog warnings without touching call sites.
- Form labels in `PermissionRulesModal`, `SkillsManagerModal`, and the new-action modal now have proper `for/id` associations.

### 🐛 Security audit
- **XSS fix** in code-block rendering: `langLabel` (AI-controlled markdown lang string) is now `textContent`-rendered, not `innerHTML`.
- 6× `unwrap()` → `map_err` in MCP serialization (no more panic on edge-case JSON).
- `npm audit` from 5 vulns (2 high) → 3 low (transitive `cookie` only — not exploitable in a Tauri webview).
- `RESET_APP` only clears Lucy-prefixed `localStorage` keys instead of `localStorage.clear()`.

### 📚 New library modules
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

---

## What's New in v1.2.0

Three sprints of work landed in this release — Lucy is now more **self-correcting**, **retrieval-aware**, and **memory-persistent** than ever.

### Sprint 1 — ReAct Self-Correction
Lucy now parses the exit code of every command she runs. When a tool fails (`FullyQualifiedErrorId`, `ParserError`, non-zero stderr…), she **must** emit a `<THOUGHT>` block stating (a) the probable cause and (b) a *different* next action — never blindly retrying the same broken command. After two identical failures she stops and asks you for guidance instead of burning tokens in a loop.

### Sprint 2 — Semantic Search over Skills & Memory (Ollama-powered)
A new vector index backs every saved skill and persistent memory. Lucy calls `<TOOL>semantic:natural language query</TOOL>` and gets back cosine-ranked hits even when the user's phrasing doesn't match the exact trigger words.

- **Local embeddings** via Ollama (`nomic-embed-text` by default) — zero cloud dependency, zero API cost.
- **Auto-indexing**: every skill you save and every memory Lucy writes is embedded in the background (fire-and-forget; Ollama downtime never blocks a save).
- **Backfill command** for existing data: `invoke('backfill_embeddings')`.
- **Graceful fallback**: if Ollama is offline, Lucy falls back to `search_runbooks` (TF-IDF) or `search_web`.

### Sprint 3 — Tiered Memory (MemGPT-style)
Lucy's memory is now split into three explicit tiers, each with the right cost/recall trade-off:

| Tier          | Storage                  | Scope                | How Lucy writes it                                   |
| ------------- | ------------------------ | -------------------- | ---------------------------------------------------- |
| **CORE**      | `memory_core` (always injected into system prompt, <2 KB) | Cross-session, always-on | `<TOOL>memory_core_set:section\|\|\|key\|\|\|value</TOOL>` |
| **WORKING**   | `memory_working` (per-session summaries) | Current session          | Auto-compression of long agent loops                  |
| **EPISODIC**  | `agent_memories` + FTS + embeddings | Cross-session, searchable | `<TOOL>memoria_guardar:title\|\|\|content\|\|\|tags</TOOL>` |

Valid CORE sections: `user_facts`, `preferences`, `rules`, `environment`. Only truly always-relevant facts should be promoted to CORE — everyday discoveries belong in episodic memory.

### UX polish
- Removed the experimental Live Trace floating panel (kept the internal ReAct trace helpers that power self-correction).
- Repository cleanup: removed ~15 stale patch/apply/orchestrator scripts and local archives.

## Demo Video

[![Lucy AI Demo](https://img.youtube.com/vi/Moo_gfYI5h8/maxresdefault.jpg)](https://www.youtube.com/watch?v=Moo_gfYI5h8)

**[Watch Full Demo on YouTube](https://www.youtube.com/watch?v=Moo_gfYI5h8)** — See Lucy in action: self-correcting agent loops, local semantic search, tiered memory, compliance checks, and full SysAdmin automation.

## Core Features: The Agentic OS

Lucy has evolved from a conversational tool into a fully **Autonomous Agentic OS**:

- **Sub-Agents & Parallel Orchestration** — Fork tasks natively to independent background agents using Ollama (Local) or Cloud models, allowing simultaneous multi-threaded execution.
- **Self-Healing Execution Loop** — If Lucy encounters an error running a command (e.g., PowerShell access denied), she intercepts the terminal output on the fly, auto-corrects her approach, and retries until success without user intervention.
- **OpenClaw TCP Gateway** — Integrated native webhook listener on port `31337`. External systems can trigger Lucy instantly, automatically spawning dedicated Agent Tabs to process the events.
- **Claude Mem (Anti-Amnesia)** — Lucy securely saves architectural memory seamlessly into `workspace_memory.md`, retaining context across sessions and preserving knowledge permanently.
- **Graphify AST Integration** — Advanced codebase parsing hooks allowing Lucy to query Abstract Syntax Trees to map logic accurately.
- **Local LLM Emancipation** — Auto-parsers convert markdown into native OS commands instantly. Restricted local models (like `llama3`, `qwen`) can operate as unrestricted SysAdmin executors, massively reducing token latency with dynamic environment injection.

### Standard SyAdmin Features

- **Remote Shell (NexShell)** — Execute commands on remote Windows and Linux hosts via SSH/WinRM
- **Log Viewer & Infrastructure Inventory** — Monitor event logs, auto-discover services, and track installed software
- **CIS Compliance** — Run strict CIS benchmark checks against Windows and Linux baselines
- **Audit Trail & Reports** — Export PDF reports and log every administrative action securely
- **Credential Vault** — Secure API key and host credential storage via OS keyring

## Screenshots

### Main Interface & Dashboard
![Setup & Dashboard](docs/screenshots/Screenshot_1_v2.png)
![Main Interface](docs/screenshots/Screenshot_2_v2.png)

### AI Chat & Features
![Chat Interface](docs/screenshots/Screenshot_3_v2.png)
![Chat Interaction](docs/screenshots/Screenshot_4_v2.png)
![Feature Settings](docs/screenshots/Screenshot_5_v2.png)

### Infrastructure & Compliance
![Inventory View](docs/screenshots/Screenshot_6_v2.png)
![Compliance Scanning](docs/screenshots/Screenshot_7_v2.png)
![Log Analysis](docs/screenshots/Screenshot_8_v2.png)

### Advanced Features
![Audit Trail](docs/screenshots/Screenshot_9_v2.png)
![Remote Shell (NexShell)](docs/screenshots/Screenshot_10_v2.png)
![Skills & Automation](docs/screenshots/Screenshot_11_v2.png)

### v1.2.0 — ReAct, Semantic Search & Tiered Memory
![ReAct Self-Correction Loop](docs/screenshots/Screenshot_12.png)
![Semantic Search with Ollama](docs/screenshots/Screenshot_13.png)
![Tiered Memory — CORE injection](docs/screenshots/Screenshot_14.png)
![Working Memory & Session Summaries](docs/screenshots/Screenshot_15.png)
![Episodic Memory with FTS + Vectors](docs/screenshots/Screenshot_16.png)
![Incident Response Mode](docs/screenshots/Screenshot_17.png)
![Cost Dashboard & Token Tracking](docs/screenshots/Screenshot_18.png)
![Permission Rules Engine](docs/screenshots/Screenshot_19.png)
![Skills Manager with Parameters](docs/screenshots/Screenshot_20.png)
![Multi-Provider LLM Config](docs/screenshots/Screenshot_21.png)
![Live Agent Execution](docs/screenshots/Screenshot_22.png)

## Prerequisites

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://rustup.rs/) >= 1.70
- [Tauri CLI](https://tauri.app/start/prerequisites/) prerequisites (WebView2 on Windows)

## Getting Started

```bash
# Clone the repository
git clone https://github.com/Phenomx64L/LucyAI.git
cd LucyAI

# Install frontend dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

The production build generates Windows installers (NSIS + MSI) in `src-tauri/target/release/bundle/`.

## How to Use Lucy

### First-run setup
1. Launch Lucy → the setup overlay appears.
2. Paste an LLM API key (Gemini / OpenAI / Anthropic / local Ollama). Stored in the OS keyring — never on disk.
3. *(Optional)* Add remote hosts (Windows over WinRM, Linux over SSH). Credentials also go in the keyring.
4. Pick your language (EN / ES). You're ready.

### Talking to Lucy
Lucy is **autonomous** — you describe an intent, she picks the right tool:

- **Local actions** (file, registry, processes, network): just ask. She writes PowerShell / WMIC / netsh / reg / cscript blocks and runs them.
- **Remote hosts**: mention the host name. She emits `<EXECUTE_REMOTE target="hostId">...</EXECUTE_REMOTE>` and the UI runs it over your configured WinRM / SSH tunnel.
- **Destructive commands** (`Stop-Service`, `Restart-Computer`, `Remove-*`, `reg delete`…): Lucy emits a `<PLAN>` card with **Execute / Dry-Run / Edit / Cancel** buttons instead of running blindly.
- **Code tasks** (read, analyze, edit files): uses native Rust tools (`readfile`, `editfile`, `analyze_code` via Tree-Sitter) — no PowerShell file I/O.

### Enabling semantic search (Sprint 2)
Lucy uses **Ollama** locally for embeddings — no cloud cost.

```bash
# One-time install (see https://ollama.com)
ollama pull nomic-embed-text
ollama serve   # usually runs as a service automatically on Windows
```

After that every skill you save and every memory Lucy writes is indexed automatically. To retro-index existing rows:

```js
await invoke('backfill_embeddings');   // from the browser console / a dev skill
```

If Ollama is offline, semantic tools silently fall back to TF-IDF / web search — nothing breaks.

### Teaching Lucy to remember (Sprint 3)
Three ways to persist knowledge, ordered from cheapest to most detailed:

1. **CORE memory** — Always-on facts injected into every system prompt. Keep it tight (<2 KB total). Lucy writes this herself when you state a stable preference (e.g. "always use PowerShell 7, never cmd").
2. **`<REMEMBER>` tags** — Personal profile facts (name, role, org, main projects). Persisted across sessions.
3. **`memoria_guardar`** — Episodic knowledge: discoveries, fixes, runbook steps. FTS + vector-searchable later via `memoria_buscar` or `semantic:` tools.

You can review and edit everything Lucy has remembered from the **Settings → Memory** panel.

### Skills & Runbooks
- Teach Lucy a reusable action with `<LEARN>trigger words|powershell command|confirmation message</LEARN>` — she'll auto-save it as a Skill.
- Parameterize with `{{var_name}}` in the command; Lucy will prompt you when needed.
- Manage / test / export everything from the **Skills** tab in the sidebar.

### Compliance, Inventory & Audit
- **Compliance** → run CIS baselines against local or remote Windows / Linux hosts, export PDF reports.
- **Inventory** → auto-discover services, installed software, hardware, NICs.
- **Audit Trail** → every command, plan execution, and memory write is logged to SQLite with a signed timestamp.

### Keyboard shortcuts
| Shortcut              | Action                        |
| --------------------- | ----------------------------- |
| `Ctrl+T`              | New agent tab                 |
| `Ctrl+W`              | Close current tab             |
| `Ctrl+K`              | Command palette               |
| `Ctrl+,`              | Settings                      |
| `Esc`                 | Stop running agent loop       |

## Project Structure

```
lucy-svelte/
├── src/                      # Frontend (SvelteKit)
│   ├── routes/
│   │   └── +page.svelte      # Main application view
│   └── lib/
│       ├── *View.svelte       # Feature views (Dashboard, NexShell, Logs, etc.)
│       ├── *Modal.svelte      # Modal dialogs (Host, Keyring, Profile, etc.)
│       ├── lucy-api.ts        # Tauri command bridge
│       ├── stores.ts          # Svelte reactive stores
│       ├── skill-engine.ts    # Skill execution engine
│       └── hooks/             # Command guard & turn loop
├── src-tauri/                 # Backend (Rust + Tauri)
│   ├── src/
│   │   ├── lib.rs             # Tauri command registration
│   │   ├── state.rs           # Application state
│   │   ├── commands/          # Command modules
│   │   │   ├── ai.rs          # LLM integration
│   │   │   ├── hosts.rs       # Remote host execution
│   │   │   ├── shell.rs       # Interactive shell
│   │   │   ├── compliance.rs  # CIS benchmark checks
│   │   │   ├── inventory.rs   # Infrastructure discovery
│   │   │   ├── logs.rs        # Log reading
│   │   │   └── ...
│   │   └── utils/             # Shell & logging utilities
│   └── tauri.conf.json        # Tauri configuration
├── static/                    # Static assets
└── package.json
```

## Tech Stack

| Layer    | Technology                         |
| -------- | ---------------------------------- |
| Frontend | SvelteKit 5, Vite 6, TypeScript    |
| Backend  | Rust 2021, Tauri 2.0, Tokio        |
| AI       | LLM via API (configurable)         |
| Security | OS Keyring, CSP headers, DOMPurify |
| UI       | Lucide icons, Highlight.js, Marked |
| Database | SQLite (tauri-plugin-sql)           |
| Reports  | jsPDF + AutoTable                  |

## Configuration

Lucy stores credentials securely in the OS keyring. On first launch, the setup overlay will guide you through:

1. Setting your LLM API key
2. Configuring your first host profile
3. Selecting your preferred language (EN/ES)

## Available Scripts

| Command                  | Description                        |
| ------------------------ | ---------------------------------- |
| `npm run dev`            | Start Vite dev server              |
| `npm run build`          | Build frontend for production      |
| `npm run preview`        | Preview production build           |
| `npm run check`          | Run svelte-check type validation   |
| `npm run tauri dev`      | Launch full app in dev mode        |
| `npm run tauri build`    | Build native desktop installer     |

## Security Considerations

- API keys are stored in the OS keyring, never in plaintext files
- All HTML rendering is sanitized with DOMPurify
- CSP headers restrict external connections to configured API endpoints
- Remote command execution requires explicit host configuration and credentials

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Commit your changes (`git commit -m 'Add my feature'`)
4. Push to the branch (`git push origin feature/my-feature`)
5. Open a Pull Request

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines.

## Support Lucy AI

Love Lucy? Consider supporting development! Your contribution helps improve the project and keeps it free and open source.

### 💝 Sponsor Options

- **[GitHub Sponsors](https://github.com/sponsors/Phenomx64L)** — Direct sponsorship
- **[Buy Me a Coffee](https://www.buymeacoffee.com/phenomx64l)** — One-time or recurring
- **[Patreon](https://patreon.com/lucy-ai)** — Monthly support
- **[PayPal](https://paypal.me/phenomx64l)** — Donation

### 🙏 Other Ways to Help

- ⭐ **Star the repository** — Helps visibility
- 🐛 **Report bugs** — Create detailed issues
- 💡 **Suggest features** — Share your ideas
- 🔧 **Contribute code** — Submit PRs
- 📢 **Share Lucy** — Tell others about it

## License

This project is licensed under the GNU General Public License v3.0 (GPLv3). See [LICENSE](LICENSE) for details.
This ensures that any modifications or derivatives of Lucy remain Open Source and credit the original author.

---

<p align="center">
  Built with Tauri + SvelteKit + Rust
</p>
