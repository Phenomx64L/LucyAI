# Contributing to Lucy AI

First off, thanks for taking the time to contribute! 🎉

Lucy AI is an open-source project and we love community contributions. This document provides guidelines and instructions for contributing.

## Code of Conduct

- Be respectful and inclusive
- Welcome diverse perspectives
- Assume good intent
- Address concerns constructively

## How Can I Contribute?

### Reporting Bugs

Found a bug? Please create an issue with:

1. **Clear title** — Summarize the problem
2. **Reproduction steps** — How to reproduce the issue
3. **Expected behavior** — What should happen
4. **Actual behavior** — What actually happens
5. **Environment** — OS, Lucy version, Node/Rust versions
6. **Screenshots** — If applicable

**Example:**
```
Title: NexShell crashes when executing PowerShell commands with pipes

Steps:
1. Open NexShell
2. Execute: Get-Process | Where-Object {$_.Memory -gt 100MB}
3. App crashes with "invalid UTF-8" error

Expected: Command output displayed
Actual: Application exits
Environment: Windows 11, Lucy v1.0.0
```

### Suggesting Features

Have a great idea? Create an issue with:

1. **Use case** — Why is this needed?
2. **Proposed solution** — How would you solve it?
3. **Alternatives considered** — Other approaches?

### Submitting Pull Requests

1. **Fork the repository**
   ```bash
   git clone https://github.com/YOUR_USERNAME/LucyAI.git
   cd LucyAI
   ```

2. **Create a feature branch**
   ```bash
   git checkout -b feature/your-feature-name
   ```

3. **Make your changes**
   - Follow code style conventions (see below)
   - Add/update tests if needed
   - Update documentation

4. **Commit with clear messages**
   ```bash
   git commit -m "feat: Add feature description

   - Detail about implementation
   - Any breaking changes noted
   - Closes #123 (if applicable)"
   ```

5. **Push and open a PR**
   ```bash
   git push origin feature/your-feature-name
   ```

6. **PR Description should include:**
   - What problem does this solve?
   - How was it tested?
   - Any breaking changes?
   - Screenshots (if UI changes)

## Development Setup

### Prerequisites

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://rustup.rs/) >= 1.70
- [Tauri CLI](https://tauri.app/start/prerequisites/)

### Getting Started

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Type checking
npm run check

# Build for production
npm run tauri build
```

### Project Structure

```
lucy-svelte/
├── src/                  # Frontend (SvelteKit + Svelte 5)
│   ├── routes/          # App pages/layout
│   ├── lib/             # Components & utilities
│   │   ├── *View.svelte     # Feature views
│   │   ├── *Modal.svelte    # Modals
│   │   ├── lucy-api.ts      # Tauri command bridge
│   │   └── stores.ts        # Svelte stores
│   └── app.css          # Global styles
│
├── src-tauri/           # Backend (Rust + Tauri 2)
│   ├── src/
│   │   ├── lib.rs           # Command exports
│   │   ├── commands/        # Command handlers
│   │   └── utils/           # Utilities
│   └── Cargo.toml       # Dependencies
│
└── docs/                # Documentation
```

## Code Style

### Frontend (Svelte/TypeScript)

- Use **2-space indentation**
- **Camel case** for variables/functions: `myVariable`
- **Pascal case** for components: `MyComponent.svelte`
- Prefer **const** over let
- Use **TypeScript** for type safety
- Add **svelte-ignore** comments only when necessary

Example:
```svelte
<script>
  let count = 0;

  function handleClick() {
    count++;
  }
</script>

<button on:click={handleClick}>
  Count: {count}
</button>

<style>
  button {
    padding: 0.5rem 1rem;
  }
</style>
```

### Backend (Rust)

- Follow **Rust naming conventions** (snake_case for functions/variables)
- Use **rustfmt** for formatting: `cargo fmt`
- Check with **clippy**: `cargo clippy`
- Add **doc comments** for public functions
- Write **tests** for new logic

Example:
```rust
/// Execute a shell command on a remote host
pub async fn execute_command(
    host: &str,
    command: &str,
) -> Result<CommandOutput, CommandError> {
    // Implementation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_command() {
        // Test implementation
    }
}
```

### CSS

- Use **CSS variables** for theming: `var(--bg)`, `var(--acc)`
- Follow **BEM naming** for component-specific styles: `.component__element--modifier`
- Use **Tailwind classes** when available
- Avoid hardcoded colors (use design tokens)

## Commit Message Format

Use conventional commits for clarity:

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types:**
- `feat` — New feature
- `fix` — Bug fix
- `docs` — Documentation
- `style` — Code style (formatting, missing semicolons)
- `refactor` — Code refactoring
- `perf` — Performance improvement
- `test` — Tests
- `chore` — Build/tooling

**Examples:**
```
feat(nex-shell): Add streaming command output
fix(compliance): Resolve CIS benchmark timeout on large inventories
docs: Update README with new features
refactor(api): Simplify Tauri command bridge
```

## Testing

### Frontend
```bash
npm run check              # Type checking
```

### Backend
```bash
cd src-tauri
cargo test                 # Run all tests
cargo test --doc          # Doc tests
cargo clippy              # Linting
```

## Pull Request Process

1. **Before submitting:**
   - [ ] Code follows style guidelines
   - [ ] Tests pass (`npm run check`, `cargo test`)
   - [ ] Commit messages are clear
   - [ ] Documentation is updated

2. **After submitting:**
   - Maintainer will review
   - CI checks must pass
   - Address review feedback
   - Squash commits if requested

3. **Merging:**
   - PRs are merged after approval and CI pass
   - Squash-and-merge strategy to keep history clean

## Documentation

- Update `README.md` for major changes
- Add/update `CHANGELOG.md` entries
- Document new commands/APIs in code comments
- Keep examples up-to-date

## Questions?

- Open an issue for questions
- Check existing issues/discussions first
- Be clear and provide context

## License

By contributing, you agree your code will be licensed under MIT (see [LICENSE](LICENSE)).

---

**Thank you for contributing to Lucy AI! 🚀**
