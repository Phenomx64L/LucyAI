# Contributing to Lucy

Thanks for taking the time. This document says how to build Lucy, what the
conventions are, and where the sharp edges are.

## Code of conduct

- Be respectful and inclusive
- Welcome diverse perspectives
- Assume good intent
- Address concerns constructively

## Reporting bugs

Open an issue with:

1. **A clear title** — what broke, in one line
2. **Steps** — how to reproduce it
3. **Expected vs actual**
4. **Environment** — Windows build, Lucy version, and whether the model was
   local (Ollama) or a cloud provider
5. **Screenshots**, when the problem is something you can see

The last one matters more than usual here. Lucy is a desktop application that
paints its own window, and several of the bugs worth reporting are visual: text
that overflows a lane, a chip that reads at the wrong contrast, a strip that
invites a click it has already consumed.

**Example:**

```
Title: NexShell loses the tail of a long command's output

Steps:
1. Open NexShell
2. Run: Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=(Get-Date).AddDays(-7)}
3. Scroll to the end

Expected: the whole table
Actual: the last few rows are cut, with no "truncated" marker
Environment: Windows 11 26100, Lucy 2.1.0, local model
```

## Development setup

### Prerequisites

- **Stable Rust** (install with [rustup](https://rustup.rs/))
- **Windows 10 or 11**

That's the whole list. There is no Node, no npm, no Tauri CLI and no WebView2
SDK: Lucy is a single Rust binary that links no browser engine. If you are
looking at an older version of this file that asked for those, that was v1.

Windows is not incidental. Lucy shells out to PowerShell, reads the Event Log,
talks to WinRM, and calls Win32 for the local clock and screen capture. The core
crate compiles elsewhere and its tests mostly pass, but the application does not.

### Building and running

```bash
cargo run --release --manifest-path lucy-native-proto/lucy-egui/Cargo.toml
```

Debug builds work and are much faster to compile, but the UI runs at a fraction
of the frame rate — several views paint thousands of rows. Develop in debug,
judge performance in release.

### Tests

```bash
cargo test --manifest-path lucy-core/Cargo.toml
cargo test --manifest-path lucy-native-proto/lucy-egui/Cargo.toml
```

709 in the core and 230 in the shell at the time of writing, plus 10 marked
`#[ignore]` — those are measurements rather than assertions, and they print
numbers instead of passing or failing. Run one with:

```bash
cargo test --manifest-path lucy-native-proto/lucy-egui/Cargo.toml \
    cuanto_cuesta -- --ignored --nocapture
```

The core's connection pool is a process-wide `OnceLock` and a handful of tests
move process state — the working directory, the current directory. The files that
need serialising take their own mutex, so the default parallel run passes; if you
ever see a database test fail once and pass on a retry, `--test-threads=1` is the
first thing to try.

### Linting

```bash
cargo clippy --all-targets --manifest-path lucy-core/Cargo.toml
cargo clippy --all-targets --manifest-path lucy-native-proto/lucy-egui/Cargo.toml
```

Both crates carry a small number of accepted warnings — 7 in the core, 13 in the
shell, not counting the two "generated N warnings" summary lines cargo prints.
Treat those as the baseline: a pull request should not add to it. Each warning
that is deliberate carries an `#[allow]` with a comment saying why.

Every command on this page is written to run **from the root of this
repository**, which is why they all carry `--manifest-path`. There is no
`Cargo.toml` at the root — `main` is an assembly of two crates that each keep
their own, so bare `cargo test` and `cargo clippy` fail here with "could not find
Cargo.toml". Inside either working repository the short forms work.

## Layout

```
lucy-core/                The shared heart. No UI: memory, consolidation,
                          pattern mining, compliance, inventory, the watcher,
                          notifications, token spend, the shell runner.

lucy-native-proto/
  lucy-egui/              The egui shell. src/main.rs is the App and the frame
                          loop; the vista_*.rs modules are one screen each;
                          i18n.rs is the string table; theme.rs the tokens.
  proto-core/             The PTY plumbing, kept separate from the UI.
  packaging/              NSIS installer and per-language MSIs.

docs/security-skills/     A catalogue of security and forensics skills, under
                          its own license and attribution.
docs/research/            Design notes on the memory system and the graph.
```

`lucy-core` and `lucy-native-proto` each have their own working repository and
their own branch here — `nucleo` and `egui`. `main` receives them by subtree
merge, so **a change to either belongs in its own repository first**. Editing the
copy under `main` produces a conflict at the next merge.

## Code style

Rust only, and mostly what `rustfmt` and clippy already enforce. What is specific
to this codebase:

**Comments, commit messages and test names are in Spanish.** The interface is
translated into five languages; the source is not. If you don't read Spanish, the
test names are written as full sentences and still tell you what each piece
guarantees.

**Comments carry the measurement, not the intention.** The convention here is
that a non-obvious decision is written down together with the number that
produced it — "medido: 8.088 caracteres estables contra 213 volátiles", "el
tope de salida de Anthropic está en 4096 sobre una petición en streaming". A
comment that says what the code does is noise; one that says what was measured
and what was rejected is why this codebase is navigable at all.

**Tests are named as the sentence they defend**, not after the function they
call: `una_chincheta_apagaba_la_busqueda_por_palabras`, not `test_recall`. When
a test exists because something broke, the comment says what broke.

**A guard has to be able to fail.** Several tests in this tree once passed while
checking nothing — reading a file that no longer existed, scanning a source file
that had been split in nine. If you add a floor (`assert!(n >= 40)`), pick a
number that catches the realistic failure, not only the catastrophic one, and say
in a comment which failure you had in mind.

## Commits

Conventional commits, in Spanish, with a subject that says the symptom rather
than the change:

```
fix(memoria): una chincheta apagaba la busqueda por palabras
perf(prompt): la marca de cache no cortaba nada y viajaba literal al modelo
```

Bodies here are long on purpose. The pattern is: what was wrong, what it broke
that nobody would have noticed, what was measured, and what was decided against.
Look at `git log` for two or three before writing your first.

Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`.

## Packaging

```powershell
cd lucy-native-proto/packaging
./build-all.ps1
```

**This does not work on a clean machine today, and it is a known gap.** The
scripts look for `makensis.exe` under `%LOCALAPPDATA%\tauri\NSIS` and for WiX
under `%LOCALAPPDATA%\tauri\WixTools314` — where the Tauri CLI used to download
them when v1 was still in the tree. With v1 gone there is nothing to download
them, and `build-all.ps1` still tells you to run a command that no longer exists.

Until that is fixed, either install NSIS and WiX 3.14 into those paths by hand,
or build the executable and skip the installer. Running the application from
`cargo run` needs none of this.

## Pull requests

1. Branch from the branch of the repository you're changing — `nucleo` for the
   core, `egui` for the shell.
2. Before opening it:
   - `cargo test` passes in the crate you touched
   - `cargo clippy --all-targets` adds nothing to the baseline
   - the commit body says what was measured
3. **There is no CI.** The pre-commit hooks left the tree along with v1 and have
   not been replaced, so nothing runs your tests but you. That is a gap, not a
   policy — until it closes, the checks above are on the honour system.

## Documentation

- Update `README.md` when the shape of the project changes
- Add a `CHANGELOG.md` entry for anything a user would notice
- If you remove code, remove the documentation that promises it. Documentation
  that describes something deleted is worse than documentation that is merely
  old: it makes people skip writing the thing that is actually missing.

## License

Lucy is **GPLv3** — see [LICENSE](LICENSE). By contributing you agree your code
is licensed under GPLv3 as well.

(An earlier version of this file said MIT. It was wrong: the LICENSE file has
always been GPLv3, and so has the badge in the README.)

The catalogue under `docs/security-skills/` carries its own license and
attribution; see
[`docs/security-skills/ATTRIBUTION.md`](docs/security-skills/ATTRIBUTION.md).
