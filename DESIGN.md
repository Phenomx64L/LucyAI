---
name: Lucy SOC
colors:
  primary:    "#10b981"
  secondary:  "#3b9eff"
  tertiary:   "#a78bfa"
  warn:       "#f59e0b"
  danger:     "#ef4444"
  bg-base:    "#0d1117"
  bg-card:    "#161b22"
  bg-elev:    "#21262d"
  text-main:  "#e2e8f0"
  text-muted: "#64748b"
  text-bright:"#f1f5f9"
  border:     "#1e293b"
  border-lt:  "#334155"
typography:
  h1:         { fontFamily: "Inter", fontSize: "1.4rem",  fontWeight: 700 }
  h2:         { fontFamily: "Inter", fontSize: "1.05rem", fontWeight: 600 }
  body-md:    { fontFamily: "Inter", fontSize: "13px",    fontWeight: 400 }
  body-sm:    { fontFamily: "Inter", fontSize: "11px",    fontWeight: 400 }
  label-caps: { fontFamily: "Inter", fontSize: "10px",    fontWeight: 600 }
  mono:       { fontFamily: "JetBrains Mono", fontSize: "12.5px", fontWeight: 400 }
rounded:
  sm: "5px"
  md: "8px"
  lg: "12px"
  pill: "999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "20px"
  xl: "32px"
motion:
  instant:    "80ms"
  fast:       "160ms"
  base:       "240ms"
  slow:       "400ms"
  deliberate: "600ms"
---

## Overview

Lucy's identity is **Operations Center grade**: a dark, dense workstation
that respects the muscle memory of SysAdmins coming from PowerShell ISE,
Wireshark, htop, and Windows Server Manager. Every visual choice answers
the question *"will this still feel right at 2 AM during an incident?"*.

The aesthetic blends three references:
- **Linear** — for the typography rigor and motion timing.
- **Cursor** — for the ambient state indicator + accent-tinted focus.
- **Warp Terminal** — for the layered glassmorphism and gradient depth.

## Colors

The palette is rooted in a single operational accent (neon green
`#10b981`) over a near-black GitHub-like base (`#0d1117`). Every other
hue is **semantic**, never decorative.

- **primary (#10b981):** the only "go / ok / connected" color. Used for
  CTAs, the status orb at idle, success badges, and verified telemetry.
  Never use it for plain text — eyes are tuned to read it as an action
  signal.
- **secondary (#3b9eff):** thinking / streaming / informational. Cyan
  is the "Lucy is working" tone — input border on streaming, reasoning
  bubble, low-severity hints.
- **tertiary (#a78bfa):** reasoning / memory / model-related metadata.
  Used sparingly so when it appears it carries meaning ("a memory was
  used", "a sub-agent forked").
- **warn (#f59e0b):** caution. Approval-required commands, executing
  state, anomaly badges (≥3σ but not extreme).
- **danger (#ef4444):** error / blocked / extreme anomaly. NEVER use
  for non-error UI — users learn this means "stop and read".

State-aware components (input bar, status orb, mesh gradient ambient)
swap their `--state-color` between primary/secondary/warn/danger
following the global `data-state` attribute on `<body>`.

## Typography

- **Inter** for all UI prose. Variable-font, weights 400-700.
- **JetBrains Mono** for code, paths, IDs, model names, command output.
  Always prefer mono for any string the user will copy/paste.

The hierarchy is intentionally compact (h1 = 1.4rem, not 2rem+) so the
sidebar + main column + chat list all fit comfortably on a 1366px laptop
without sacrificing legibility.

## Motion

A unified token system, never hand-roll a duration:
- **instant (80ms)** — focus rings, color shifts. Sub-perceptual.
- **fast (160ms)** — buttons, toggles, hover states.
- **base (240ms)** — element appearance, layout shifts within a panel.
- **slow (400ms)** — view transitions, sidebar collapse.
- **deliberate (600ms)** — modal entrances with spring overshoot.

Use `--ease-out` for natural deceleration, `--ease-spring` only when the
overshoot reads as celebratory (success states, modal pops).

## Density

Lucy is intentionally **dense**. SysAdmins want signal-per-pixel, not
whitespace-as-aesthetic. When in doubt, reduce padding before reducing
information.

Exceptions where breathing room is mandatory:
- The first-run `SetupOverlay` — it's the first impression.
- Modal dialogs — they read as ceremonial moments.
- The Dashboard hero cards — numbers want vertical space.

## Glow + Atmosphere

The mesh gradient ambient (radial gradients drifting at 40s cycle) is
a deliberate "this app is alive" cue. Keep its opacity ≤0.45 so it
never competes with content. The ambient color tracks `--state-color`
so the entire window subtly tints with Lucy's mood.

State glow on the input bar follows the same `--state-color`. Outer
shadow capped at 12px so it reads as a presence, not a halo.

## What NOT to do

- Don't introduce new accent colors. The 5 semantic hues above are the
  whole palette.
- Don't use sentence case on uppercase labels (`AUDIT TRAIL`, not
  `Audit Trail` — we're an ops console, not a marketing site).
- Don't animate without a motion token. Hand-rolled `0.15s ease` is
  the surest way to make the app feel inconsistent over time.
- Don't use emoji as primary iconography. Tabler icons + the small
  unicode set already in `LEGACY_ICON_MAP` is the canonical source.
