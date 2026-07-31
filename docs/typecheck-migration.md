# Bringing the untyped half of the codebase under type-checking

> Started 2026-07-29. Live document — update the count when you move it.

## Why

`npm run check` runs `svelte-check` with `checkJs: false`. Its green "0 errors"
therefore covers only files with a **typed** script block. Everything on plain
`<script>` has never been looked at — including the three biggest files in the
app:

| File | Lines | Checked before this migration? |
|---|---|---|
| `src/routes/+page.svelte` | 14 500 | ❌ |
| `src/lib/cockpit/CockpitShell.svelte` | 1 434 | ❌ |
| `src/lib/cockpit/AgentWorkspace.svelte` | 691 | ❌ |

That is the whole Terminal IA surface, ~16.6 kLOC, invisible behind a green
badge. The blind spot shipped real defects, every one of which this migration's
config catches:

- `/compare` wrote to the return of `addMsg`, which had no `return` statement —
  the results were discarded and the placeholder spun forever.
- `skillId` read `.id` off a `SecuritySkillFull`, whose id lives under `.meta` —
  the context chip reported "no skill" with one loaded.
- The cockpit's `atts` mirror type still described its pre-v1.8.1 shape.
- `esc` was used ~2 000 lines above its declaration (found earlier, same class).
- 84 icons passed `strokeWidth`, a prop that does not exist on them.

## The target, and why it is not full strict

Three configurations, measured on 2026-07-29:

| Config | Errors | What it buys |
|---|---|---|
| `checkJs: false` (today's `npm run check`) | 0 | Typed files only |
| **`checkJs: true` + `strict: false`** | **217** | **Every defect listed above** |
| `checkJs: true` + `strict: true` | 2 498 | The above + `noImplicitAny` |

The 2 281-error difference is almost entirely unannotated callback parameters.
**Not one of the defects above needed `noImplicitAny` to be caught.** Paying ten
times the cost for none of the bugs is the wrong trade, so the target is the
middle row.

The `.ts` files keep full `strict` from `jsconfig.json`. `jsconfig.checkjs.json`
is an **additional** pass, not a replacement — relaxing a rule never turns a
passing file red.

```
npm run check      # existing gate: strict, typed files only     — must stay 0
npm run check:js   # this migration: checkJs, relaxed            — driving to 0
```

## Progress

| Date | Errors | Files | What moved |
|---|---:|---:|---|
| 2026-07-29 | 217 | 36 | Baseline |
| 2026-07-29 | 128 | 21 | Phase 1 — `strokeWidth` → `stroke` |
| 2026-07-29 | 115 | 12 | Phase 2a — the defects outside the monolith |
| 2026-07-29 | **80** | **11** | Phase 2b — `app.d.ts` globals + `chatInput()` |

### Phase 1 — the icon prop (done)

89 of the 217 errors, across 22 files, were one mistake repeated: `strokeWidth`
passed to a Tabler icon. `IconProps` declares `stroke`, not `strokeWidth`, and
Svelte does not camelCase SVG attributes the way React does — so the prop landed
in `$$restProps` as a literal `strokeWidth` attribute the browser ignores, and
the component's own `'stroke-width': stroke` spread (which comes *after*
`$$restProps`) overwrote it regardless.

Every one of those 84 icons rendered at the default stroke of 2. The 25 that
asked for 1.8 / 1.9 / 2.5 were visibly wrong; the rest were accidentally right.
The repo already used `stroke={…}` correctly in ~200 other places — this
unified it.

### Phase 2a — the defects outside the monolith (done)

Two of the 26 were user-visible bugs that only a type-checker finds, because
both read perfectly:

- **`LucyCombobox` never filtered.** It read the typed text from
  `<Combobox.Root bind:inputValue>`, but bits-ui documents that prop as
  read-only and does not declare it `$bindable()`. A `bind:` to a non-bindable
  prop in Svelte 5 is one-way, so the local `inputValue` stayed `''` and the
  filter below it always returned the whole list. Reads from the input's own
  `oninput` now.
- **`setSecuritySkillAsPreset(full, true)`** passed a second argument to a
  one-parameter function. JS drops it silently, so whatever the `true` was
  meant to switch on has never happened. The other call site always passed one.

The rest were shape and annotation fixes: three `export let x = null` props
whose type TS infers as literally `null` (annotated with JSDoc rather than
silenced), and `JSON.parse(reader.result)` where the result is
`string | ArrayBuffer | null`.

**On the five `@ts-expect-error` directives** reported as unused: they are NOT
dead. They are required by the strict gate and only unused here because
`strict: false` disables `strictNullChecks`. Replacing them with an explicit
`as any` cast satisfies both configs; swapping in `@ts-ignore` would have
traded a conditional silencer for an unconditional one. Re-enabling
`strictNullChecks` in this config was measured at **463 errors** — a separate
and also worthwhile axis, but not this migration's.

### Phase 2b — the monolith's DOM noise (done)

Two systematic fixes, 35 errors, no behaviour change:

- **`src/app.d.ts`** declares the globals Lucy installs on `window` —
  `__lucyFreezeLog`, `_lucyRunFix`, `selectRunbooksDir`, `__lucyCheckpoints`,
  `_lucyJumpToMemoryId`, `__lucy_capture_evidence`, plus the Web Speech API
  that TypeScript's DOM lib still omits. 17 errors, and more usefully it is now
  the only written record of that surface. Keep the types conservative: a
  declaration that overstates what a global holds invents guarantees the
  assignment site never made.

- **`chatInput()`** replaces `document.querySelector('.chat-wrap.on .ibox')`,
  which appeared verbatim **19 times**. `querySelector` returns `Element`,
  which has no `focus()`, so 14 of those were type errors. Two spots narrowed
  correctly with `instanceof HTMLElement` and the rest called `?.focus()`
  straight off the Element — the file held both patterns at once. One accessor
  fixes the type and gives the selector a single home.

The lesson from both phases holds: **group by message before editing.** Phase 1
was 89 errors and one `sed`; this was 35 and two edits. Going file by file
would have been days of work for the same result.

### Remaining, in suggested order

1. **The last ~13 outside `+page.svelte`** — library-typing friction rather
   than defects: uPlot wanting `TypedArray`, d3's `SimulationNodeDatum`, prop
   unions typed as `string`. Each wants a narrow local type, not a cast.
2. **`+page.svelte` (102)** — the long tail. Triage notes for the property
   errors are in the commit message of `f1c2191`; the DOM-lib ones
   (`EventTarget`, `Element`, custom `Window` globals) are noise and want a
   small ambient `.d.ts` rather than 40 individual casts.
3. **Flip `checkJs: true` in `jsconfig.json` and delete
   `jsconfig.checkjs.json`** — once the count is 0, one gate is better than two.
4. **Wire it into CI** the moment it reaches zero. An ungated check rots back
   to red; that is exactly how the original blind spot lasted this long.

## Rules while migrating

- **Never silence with `@ts-ignore`.** Every error in this count is either a
  real defect or a type that does not describe reality. Both are worth fixing;
  neither is worth hiding. If something genuinely cannot be typed, widen the
  declaration and say why in a comment.
- **Fix the type, not the call site**, when the type is the thing that is wrong.
  The `atts` mirror is the model: the renderer and all three senders were
  right; the interface was four fields behind.
- **A repeated error is one fix, not N.** Phase 1 was 89 errors and a single
  `sed`. Group by message before you start editing.
