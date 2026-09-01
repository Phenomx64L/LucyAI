<p align="center">
  <img src="icon.png" alt="Lucy" width="120" />
</p>

<h1 align="center">Lucy</h1>

<p align="center">
  <strong>A SysAdmin assistant for Windows</strong><br>
  Native UI. No embedded browser.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-2.1.0-7dd3fc" alt="v2.1.0" />
  <img src="https://img.shields.io/badge/egui-0.29-blue" alt="egui 0.29" />
  <img src="https://img.shields.io/badge/Rust-2021-brown?logo=rust" alt="Rust 2021" />
  <img src="https://img.shields.io/badge/license-GPLv3-green" alt="GPLv3" />
</p>

---

## What it is

Lucy administers Windows machines. It reports how they're doing, runs what you
ask it to in plain language, audits CIS compliance, reads event logs, takes
inventory, and remembers what it has learned about each box. It talks to local
models through Ollama or to cloud providers, and in private mode nothing leaves
the machine.

## Why this version exists

**19.6 MB against 213 MB.** That's the whole migration in one line.

V1 was Tauri 2 + SvelteKit on top of WebView2. It worked, and it dragged an
entire browser engine along to paint a grid of cards and a process table. This
version paints the same thing with [`egui`](https://github.com/emilk/egui) in a
single executable that links no browser at all.

The old installer weighed 213 MB. This one weighs 19.6 MB, and the difference is
exactly the engine that is no longer there.

## Layout

```
lucy-core/            The shared heart. No UI and no Tauri: memory,
                      consolidation, pattern mining, compliance, inventory,
                      the watcher, notifications, token spend.

lucy-native-proto/    The native face.
  lucy-egui/            The egui shell: the eight modules and the string table.
  packaging/            NSIS installer and MSI.

docs/security-skills/ A catalogue of security and forensics skills, under its
                      own license and attribution. Lucy reads skills from the
                      user profile, not from here — this is where to copy them
                      from.

docs/research/        Design notes on the memory system and the knowledge graph.
```

Both projects were brought in with `git subtree`, so their full history is in the
log: `git log -- lucy-core` shows how each piece got there.

Each one still has its own working repository, and its own branch here:

| directory           | branch   |
| ------------------- | -------- |
| `lucy-core`         | `nucleo` |
| `lucy-native-proto` | `egui`   |

To pull into `main` whatever was pushed to one of them:

```bash
git subtree pull --prefix=lucy-core origin nucleo
```

## The eight modules

| Module            | What it does                                                       |
| ----------------- | ------------------------------------------------------------------ |
| **Dashboard**     | CPU, RAM, disks, network, stopped services, and history trends      |
| **Terminal IA**   | Ask in plain language; it proposes the command and runs it on your approval |
| **NexShell**      | A real PowerShell, local or remote over WinRM                       |
| **Log Viewer**    | What ran, what came back, and how long it took                      |
| **Inventory**     | Listening ports, services, installed software, certificates         |
| **Compliance**    | CIS controls, with the evidence behind every verdict                |
| **Memory**        | Facts, distilled sessions, ingested manuals, standing principles    |
| **Settings**      | Keys, models, thresholds, language, appearance                      |

## Languages

Spanish, English, Portuguese, French and German.

The Spanish string **is the key** into the translation table, which is looked up
by binary search. A test enforces that the table stays sorted, because an
unsorted table doesn't fail — it just silently stops finding half the strings.

## A note for contributors

**Code comments, commit messages and test names are in Spanish.** The user
interface is fully translated; the source is not, and it is unusually
comment-heavy on purpose — most non-obvious decisions carry the measurement that
produced them. If you read Spanish, that's the fastest way into the codebase. If
you don't, the tests are named as sentences and still tell you what each piece
guarantees.

## Building

Requires stable Rust and Windows 10/11.

```bash
cargo run --release --manifest-path lucy-native-proto/lucy-egui/Cargo.toml
```

Tests for each half:

```bash
cargo test --manifest-path lucy-core/Cargo.toml
cargo test --manifest-path lucy-native-proto/lucy-egui/Cargo.toml
```

574 assertions in the core and 201 in the shell, all green.

## Where V1 went

All of it is still in this repository. Nothing was removed from history:

- The **`v1-svelte-final`** tag points at its last complete tree.
- The 48 `v1.x` tags mark every published release.
- `git show v1-svelte-final:src-tauri/src/main.rs` still works.

What was taken out of `main` was the code and its build scaffolding — not its
record.

---

## Author

**Iván Eduardo Luna** ([@Phenomx64L](https://github.com/Phenomx64L))
· [LinkedIn](https://linkedin.com/in/phenomx64l)
· SysAdmin and developer

Lucy came out of real infrastructure administration problems. Every
architectural decision comes from having had one of them in front of me.

## License

GPLv3 — see [LICENSE](LICENSE).

The catalogue under `docs/security-skills/` carries its own license and
attribution; see
[`docs/security-skills/ATTRIBUTION.md`](docs/security-skills/ATTRIBUTION.md).
