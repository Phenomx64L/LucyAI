# Setup — Lucy Assistant

Two audiences, one document:

- **§1 Run-only** — you just want Lucy installed and working.
- **§2 Develop** — you want to build from source, run tests, ship installers.

If anything below is wrong on your machine, please open an issue at
https://github.com/Phenomx64L/LucyAI/issues — these requirements drift
faster than the rest of the docs.

---

## §1 Run-only requirements

For users who just want a working Lucy on a Windows machine.

### Mandatory

| Requirement | Why | Where to get |
|---|---|---|
| **Windows 10 1809 / Windows 11** (x64) | Lucy is Windows-first; WebView2 + several SysAdmin tools (Get-Service, registry, WinRM) are Win-only. | Built in |
| **Microsoft WebView2 Evergreen Runtime** | Tauri renders the UI inside WebView2. | **Bundled in the installer** (~120 MB). On Win11 + recent Win10 it's already installed. |
| **~250 MB free disk space** | Install footprint + WebView2 runtime if it had to install. | — |

### Strongly recommended (Lucy degrades gracefully without them, but loses features)

| Requirement | Why | Get it |
|---|---|---|
| **At least one LLM API key** | Otherwise Lucy can't talk to a cloud model. Configure inside Lucy → Settings → Provider Config. Keys are stored in Windows Credential Manager (encrypted). | [Anthropic Console](https://console.anthropic.com), [Google AI Studio](https://aistudio.google.com), [OpenAI](https://platform.openai.com), [NVIDIA NIM](https://build.nvidia.com/) |
| **Ollama** | Required for: privacy mode, local embeddings (memory recall), local sub-agents. Without it Lucy falls back to cloud-only. | https://ollama.com — install + run `ollama pull qwen2.5:7b` (or any model you prefer for the agent loop) and `ollama pull nomic-embed-text` for memory embeddings. |
| **PowerShell 5.1+** (already on every modern Windows) | Lucy's primary execution engine. | Built in |
| **OpenSSH client** (Windows optional feature) | For Lucy's remote multi-host management over SSH. | `Settings → Apps → Optional features → OpenSSH Client` |

### Optional (only if you'll use these features)

| Feature | Extra requirement |
|---|---|
| **Remote Linux hosts via SSH** | OpenSSH client (above). Optional private-key files. |
| **Remote Windows hosts via WinRM** | WinRM enabled on target hosts. Lucy's `validate_host` will run a preflight check before connecting. |
| **Multi-factor / Active Directory** | Lucy works with current user creds; for service-account scenarios, configure in Settings. |
| **PromptGuard 2 ML guard** | `--features ml-guard` build + download `meta-llama/Llama-Prompt-Guard-2-86M` ONNX (~340 MB, gated by Meta license). Without it, regex-only guard runs. |
| **Cross-encoder reranker** | `--features ml-reranker` build + run `/reranker-install` slash command to download `cross-encoder/ms-marco-MiniLM-L-6-v2` (~22 MB, public). Without it, RRF-only ranking. |

### Where Lucy stores stuff

```
%APPDATA%\com.lucy.dev\
├── lucy.db              SQLite (memories, crystals, insights, runbooks, audit, …)
├── lucy.db-wal
├── secrets.json         encrypted MCP secrets
├── logs\                rolling structured logs
└── checkpoints\         in-flight agent state (for recovery after crash)
```

Lucy will create this on first launch — no manual prep needed.

### Quick verification after install

1. Launch Lucy from the Start Menu.
2. First-run dialog asks for your name + an LLM provider.
3. In the chat type: `hola Lucy`. You should get a greeting.
4. Type `Get-Service | Select -First 3`. You should see real service rows (not `(sin salida)`). If you see `(sin salida)`, refer to `CHANGELOG.md`
   v1.4.0 critical-4 fix — your installer is pre-fix.

---

## §2 Developer environment

For building Lucy from source, running tests, shipping installers.

### Toolchain

| Tool | Version | Notes |
|---|---|---|
| **Rust** | stable, `1.81+` | `rustup default stable && rustup update`. Edition 2021. |
| **Node.js** | `20.x LTS` (or 22.x) | `winget install OpenJS.NodeJS.LTS`. We use ESM. |
| **npm** | comes with Node | — |
| **Visual Studio Build Tools** | 2022 with "Desktop development with C++" workload | Needed by Rust on Windows for the MSVC linker. Without it, `cargo build` fails on link. |
| **Windows SDK** | `10.0.22000+` (installed by VS Build Tools above) | — |
| **Tauri CLI** | `2.x` | Installed transitively via `npm install`. Direct: `cargo install tauri-cli --version "^2.0"`. |
| **Git** | any recent | `winget install Git.Git`. |

### Optional dev tools

| Tool | When you need it |
|---|---|
| **WiX Toolset 3.x** | Building MSI installers (`cargo tauri build` does it automatically if installed). `winget install WiXToolset.WiXToolset` |
| **VSCode / Cursor / Zed** | Editor of choice. Recommended extensions: rust-analyzer, Svelte for VSCode, Tauri, Tabnine/Copilot if you're into that. |
| **Python 3.10+** | Only for regenerating the installer BMP banners (`scripts/gen_installer_banners.py` uses Pillow). |
| **`gh` CLI** | Optional, for pushing GitHub releases from terminal. `winget install GitHub.cli` |

### First-time setup on a new machine

```powershell
# 1. Clone
cd X:\Rust_Projects     # or your workspace root
git clone https://github.com/Phenomx64L/LucyAI.git lucy-svelte
cd lucy-svelte

# 2. JS deps
npm install

# 3. Install pre-commit hooks (CRITICAL — these enforce the contract tests
#    that prevent the (sin salida) bug from regressing. See CHANGELOG.md
#    entry for v1.4.0 critical-4.)
npm run hooks:install

# 4. Verify the Rust toolchain compiles the project
cargo check --manifest-path src-tauri/Cargo.toml

# 5. Run the contract test suite (should be ~52 tests, ~2-3 s)
npm run test:contract

# 6. Build the SvelteKit frontend (sanity check)
npm run build
```

If all 6 steps succeed: you're ready to develop.

### Daily workflow

| Task | Command |
|---|---|
| **Run Lucy in dev mode** (hot reload UI, watch Rust) | `npm run tauri dev` |
| **Build release binary** (no installer) | `cargo tauri build --no-bundle` |
| **Build installer** (.exe NSIS + .msi WiX) | `cargo tauri build` — outputs in `src-tauri/target/release/bundle/` |
| **Run all Rust unit + contract tests** | `npm run test:contract` |
| **Run frontend type check** | `npm run check` |
| **Run frontend Vitest** | `npm test` |
| **Regenerate TS types from Rust** | `cd src-tauri && cargo test export_bindings` |
| **Format Rust** | `cargo fmt --manifest-path src-tauri/Cargo.toml` |
| **Lint Rust** | `cargo clippy --manifest-path src-tauri/Cargo.toml` |
| **Toggle ML guard / reranker features** | Build with `--features ml-guard` or `--features ml-reranker`. Default release omits both. |

### Pre-commit hook (don't disable it)

The hook at `.githooks/pre-commit` blocks any commit that:

1. Breaks the **contract tests** in `src-tauri/src/commands/shell.rs::tests` or `local.rs::tests`. These guard the `(sin salida)` regression and WMIC/REG misroutes.
2. Fails `cargo check` (if any `.rs` changed).
3. Adds NEW `svelte-check` errors beyond the baseline of 20 (legacy debt).

Bypass syntax exists (`git commit --no-verify`) but **document the reason** in
the commit body if you use it. The hook runs in ~2-3 s warm cache.

### Known gotchas

These have bitten real engineers; calling them out here saves you the same
half-day each.

1. **`.spawn()` without `.stdout(Stdio::piped())`** silently discards child
   output on Windows GUI apps. This was the `(sin salida)` bug; see
   `b35fc28`'s commit body. The contract tests catch it now.
2. **PowerShell with `-Command`** re-tokenises args; nested `{}` from
   Where-Object scriptblocks confuse the parser. Use a temp `.ps1` file with
   `-File` instead (we do).
3. **Tabler icon imports** must be direct path imports: `import Foo from
   '@tabler/icons-svelte/icons/foo'`. The old `{ IconFoo as Foo } from
   '@tabler/icons-svelte'` aliased syntax silently breaks tree-shaking in
   Svelte 5.
4. **Svelte 5 reactive `$:` watching object props**: writing `$: if (tab &&
   tab.foo) ...` re-fires every time the PARENT reassigns `tab` (which
   happens ~30×/agent-turn). Cache the value or watch a primitive instead.
5. **WebView2 on locked-down corp boxes** with no internet: build the
   installer with `webviewInstallMode: "offlineInstaller"` (our default).
   That bundles the runtime so first-launch works without download.
6. **r2d2 SQLite pool max_size**: default is 10, fine for prod. In tests
   we use `max_size=1` so all calls hit the same in-memory DB.

### CI/test invariants

If any of these change, update both `package.json` scripts AND `.githooks/
pre-commit`:

- The Rust unit suite (`cargo test --lib`) must stay under 10 s. Currently
  ~2-3 s.
- The contract tests in `shell::tests` and `local::tests` must NEVER be
  marked `#[ignore]`. They are the regression net.
- Tokio `macros` feature is required for `#[tokio::test]` — locked in
  `Cargo.toml`.

### Where to look when something's confusing

| What you want to understand | File |
|---|---|
| Overall architecture | `DESIGN.md` |
| Memory pipeline (Tier 1-3 cherry-picks) | `CHANGELOG.md` entries for v1.4.0 |
| How the agent loop works | `src/routes/+page.svelte::runAI()` (yes, it's big) |
| Tool dispatch | `src/lib/page/slash-commands.ts` + `runAI` TOOL regex matchers |
| Telemetry / "what is Lucy doing right now" | `src/lib/liveTrace.ts` + `LiveTracePanel.svelte` (Alt+T) |
| Why X broke | `git log --oneline` — every fix has the symptom in the subject |
| Installer | `INSTALLER.md` |
| Per-session context that survived | `%APPDATA%\com.lucy.dev\lucy.db` table `agent_session_summaries` |

---

## Moving development between machines

When carrying work to a fresh machine (e.g., a laptop), what travels:

| Layer | What | Where |
|---|---|---|
| **Source of truth** | The git repo | GitHub `Phenomx64L/LucyAI` — `git clone` and you have everything that mattered |
| **In-progress chat with Claude Code** | The `.jsonl` session file | `%USERPROFILE%\.claude\projects\<project-slug>\<session-id>.jsonl` |
| **Project auto-memory** (Claude Code MEMORY.md) | The `memory/` directory | `%USERPROFILE%\.claude\projects\X--Rust-Projects-lucy-svelte\memory\` |
| **Lucy's persisted state** (memories, crystals, insights, runbooks) | `lucy.db` (SQLite) | `%APPDATA%\com.lucy.dev\lucy.db` — copyable but rarely needed; you'll be testing fresh anyway |
| **API keys** | Windows Credential Manager | Per-machine; you'll re-add them in the new install |

For a transfer-then-continue workflow see `docs/HANDOFF_*.md` (the latest one
describes the most recent session's state).

---

## Help / contact

- Issues: https://github.com/Phenomx64L/LucyAI/issues
- Discussions: same repo, Discussions tab
- Author: Iván Eduardo Luna (Phenomx64L)
