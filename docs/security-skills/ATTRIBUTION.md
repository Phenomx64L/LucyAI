# Cybersecurity Skills Library — Attribution

## Source

These 213 cybersecurity skills are bundled from the
**Anthropic-Cybersecurity-Skills** project by **mukul975 / Mahipal**:

- Repository: https://github.com/mukul975/Anthropic-Cybersecurity-Skills
- Author: Mahipal Singh (mukul975)
- Standard: [agentskills.io](https://agentskills.io)
- Version imported: 2026-05 snapshot (commit at time of integration)
- License: **Apache License 2.0** — see `LICENSE` file in this directory

## What we bundled

For each skill in the source repo, only the **`SKILL.md`** file was
copied. We did **not** bundle:

- `references/api-reference.md` — per-skill API references (kept in
  source repo for users who clone it directly)
- `scripts/agent.py` — per-skill Python agent prototypes (Lucy uses
  its own Rust+TS agent loop)

This keeps the Lucy bundle to ~1.94 MB of pure markdown content
while preserving the full `When to Use`, `Workflow`, `Key Concepts`,
`Tools & Systems`, and `Common Scenarios` sections of every skill.

## What Lucy does with these skills — today, nothing automatic

**This catalogue ships as reference material, not as a wired feature.** It is
here to be read, and to be copied into the user profile where Lucy does read
skills from.

`/sec-skill` appears in the command palette — `main.rs:2015` registers it as
`("/sec-skill", "Catálogo security/forensics (200+)", false)`, and that `false`
is the flag that means *not implemented yet*. The palette deliberately lists
commands that don't work rather than hiding them, so the operator discovers what
Lucy is growing towards; picking one says so instead of doing nothing.

This section used to describe a Rust module —
`src-tauri/src/commands/security_skills.rs` — that walked this directory at boot
and built a search index, and a `/sec-skill <query>` that searched it. **No such
module exists in any of the three repositories**, and `src-tauri` itself left the
tree with v1. It described the intended design as though it were shipped, on a
page the root README links to.

The intent stands and the mapping work below is real. What is not real yet is the
loader.

## Five frameworks mapped

Each skill carries cross-framework references for compliance work:

| Framework | Coverage |
|-----------|----------|
| MITRE ATT&CK v18 | 14 tactics · 200+ techniques |
| NIST CSF 2.0 | 6 functions · 22 categories |
| MITRE ATLAS v5.4 | 16 tactics · 84 techniques (AI/ML threats) |
| MITRE D3FEND v1.3 | 7 categories · 267 techniques |
| NIST AI RMF 1.0 | 4 functions · 72 subcategories |

## License reminder

Apache 2.0 grants:

- Free use, modification, distribution
- Patent grant from contributors
- Required: preserve copyright notice (LICENSE file in this dir),
  state changes in modified copies

We made **no modifications** to any SKILL.md file. The library is
shipped as-imported from upstream commit at integration time.

## Updating

To refresh the skills library from upstream:

```powershell
# In an empty temp dir:
git clone https://github.com/mukul975/Anthropic-Cybersecurity-Skills

# Copy fresh SKILL.md files into Lucy's docs/security-skills/:
# (overwriting; LICENSE and ATTRIBUTION.md are preserved)
Copy-Item -Recurse Anthropic-Cybersecurity-Skills\skills\*\SKILL.md `
    docs\security-skills\ -Force
```

Track the upstream commit hash in the `Version imported` line at the
top of this file so future devs know what they're working from.
