# Lucy — R&D Roadmap

> Honest, prioritized proposals for taking Lucy from "useful SysAdmin
> assistant" to "indispensable cognitive partner". Written from a R&D
> mindset: every idea here has a concrete *why now*, an estimated *cost*,
> and an *honest risk* section. None of this is marketing copy.

---

## 0 · Reality check (where Lucy stands today)

What's already shipped and works:

| Layer | Capability |
|---|---|
| **Memory** | Tiered (CORE / WORKING / EPISODIC), frozen-snapshot pattern, atomic delete + consolidate, smart-digest LLM compaction, token-budget pruning |
| **Cognition** | ReAct self-correction, anti-amnesia (raw goal recovery), multi-intent prompt detection, verifier sub-agent (Plan C), context-compressor v3 with MD5 dedup + smart-collapse + anti-thrashing |
| **Tools** | 20+ native tools, MCP plugins, fork_task parallel sub-agents, semantic embeddings (Ollama), PDF intelligence, fuzzy matching |
| **Automation** | Skill Factory (auto-detect workflows), Principles (durable behavior rules), Scheduled Tasks (background ticker, cron parser) |
| **Observability** | Anomaly detection (z-score), cost predictor, audit trail, growth tracking via working memory |
| **Hardening** | XSS-safe rendering, blocklist obfuscation (obfstr), boot-time integrity check, release LTO/strip, prompt budget enforcement |

What it does **not** do (yet) — the white space below.

---

## 1 · Cognitive maximization (the brain)

### 1.1 ★★★★★ **Plan-then-Execute with explicit gates**
**Why now:** Lucy reasons inside a single agent loop. For a multi-step
infra change ("migrate IIS app pool then rotate certs then validate")
she dives in and corrects mid-flight. Failure mode: she may execute
step 3 before realizing step 1 was wrong, leaving the system in a
half-state.

**Proposal:** Two-phase loop.
1. **Plan phase** (no execution): Lucy emits a `<MISSION>` tag with N
   `<STEP>` items, each with `intent`, `cmd`, `verify`, `rollback`.
2. **Confirm phase**: UI renders the plan as a checklist; user clicks
   *Execute*, *Edit*, or *Cancel*.
3. **Execute phase**: each step runs in order, sub-agent verifies
   before advancing. Step failure → automatic rollback + halt.

**Implementation:** `<PLAN>` tag already exists for single destructive
ops; expand to N-step missions. Persist to existing `incidents` table
(reuse the SRE machinery).

**Cost:** ~3 days. **Risk:** medium — needs UX iteration so the plan
card doesn't feel like ceremony for trivial requests.

### 1.2 ★★★★★ **Self-Critique loop (Reflexion-style)**
**Why now:** the verifier sub-agent (Plan C) reviews the *final answer*
but doesn't iterate. Anthropic's research shows that 1-2 cycles of
"self-critique → revise → critique again" lift task accuracy 15-30%
on complex problems.

**Proposal:** After the agent produces a candidate answer, run an
internal `<REFLECT>` pass with a smaller/cheaper model:
- Did I address every part of the user's request?
- Are there contradictions with prior turns?
- Are there obvious failure modes I didn't consider?

Bake the critique back into a single revision pass. Cap at 2 cycles
to avoid runaway cost.

**Cost:** ~1 day. **Risk:** low — the verifier path already exists,
this just extends it. **Gain:** measurable quality lift on long
multi-step requests.

### 1.3 ★★★★ **Intent classifier as a separate cheap model call**
**Why now:** today Lucy's first turn does *everything* — classify
intent, pick tools, plan, execute. That conflates the
"is this trivial or complex?" decision with the actual response,
which means trivial questions still pay the full system-prompt cost.

**Proposal:** Pre-flight 1-line classifier (Gemini Flash or local
Ollama):
```
TRIVIAL  → bypass agent loop, single response
TOOL_USE → standard agent loop
PLANNING → force plan-first phase (1.1)
DESTRUCTIVE → force PLAN/VERIFY/ROLLBACK gates
```
Latency budget: 100-200 ms. Saves 3-5× on the trivial path.

**Cost:** half day. **Risk:** low.

### 1.4 ★★★★ **Conversation-scoped fact graph**
**Why now:** Lucy remembers facts atomically (key/value). She can't
answer "what hosts have I touched with PowerShell 7 errors this
week?" without re-scanning the audit log because facts are isolated.

**Proposal:** A lightweight in-process triple store
(subject-predicate-object) populated automatically:
- `host:AD-01 → has_role → DomainController`
- `host:AD-01 → had_error → NetLogon-5719 (2026-04-25)`
- `script:rotate-certs.ps1 → modifies → host:AD-01`

Queryable in 1ms. Persisted in SQLite as `fact_triples (subj, pred,
obj, source, ts)`. Lucy emits `<FACT>` tags during normal answers; a
background task indexes them.

**Cost:** ~2 days. **Risk:** medium — needs careful prompt design so
Lucy doesn't over-emit facts.

### 1.5 ★★★ **Curiosity loop — Lucy asks back when uncertain**
**Why now:** Lucy's failure mode on ambiguous prompts is to *guess*
the most likely interpretation. A thoughtful colleague asks for
clarification when the cost of the wrong guess is high.

**Proposal:** When intent classifier says `confidence < 0.7` AND the
likely action is destructive, Lucy emits a `<CLARIFY>` tag with 2-3
candidate interpretations as quick-reply chips.

**Cost:** ~half day. **Risk:** low. **Tradeoff:** more chips = higher
friction. Cap at 3 and only when confidence is genuinely low.

### 1.6 ★★★ **"Why did you do X?" — explainability replay**
**Why now:** when Lucy makes a non-obvious choice, the reasoning is
buried in the THOUGHT bubbles which collapse after the turn.

**Proposal:** A `/why` slash command that pops the most recent
agent's last `<THOUGHT>` block + the tool-call trace as a focused
read-only view. "She chose tool X because…" is one click away.

**Cost:** 4 hours. **Risk:** zero.

---

## 2 · Functional maximization (the body)

### 2.1 ★★★★★ **Cost-aware model routing**
**Why now:** Lucy already has cost predictor + multi-provider
support. She doesn't yet *choose* the cheapest viable model for
each turn. A 5-line "rephrase this" goes through Claude Sonnet at
$0.015/1k when Gemini Flash at $0.0005 would have been fine.

**Proposal:** Auto-route based on intent classifier + estimated
token budget:
- Trivial Q&A → cheapest available
- File edit / code generation → mid-tier (Gemini Pro / Sonnet)
- Multi-step reasoning > 3 tools → top-tier
- User can override per-tab

**Cost:** ~1 day. **Risk:** low. **Gain:** 50-80% cost reduction
for typical sessions.

### 2.2 ★★★★ **Live host inventory diff**
**Why now:** the `Inventory` view scans on demand. SysAdmins want
"what changed since last week on PROD-AD-01?" — patches installed,
services that disappeared, new ports opened.

**Proposal:** Inventory snapshots run automatically (via the new
Scheduled Tasks system!) and are diffed against the previous one.
A "Drift" panel shows red/green deltas. Doubles as a poor-man's
intrusion-detection signal.

**Cost:** ~2 days. **Risk:** low — leverages existing inventory +
scheduled task plumbing.

### 2.3 ★★★★ **Native multi-host broadcast as first-class concept**
**Why now:** Lucy already broadcasts to N hosts via the multi-host
modal. But the *result* is N independent panes. She can't easily say
"on which of these 12 hosts did the patch fail?".

**Proposal:** New `Broadcast` view: one command, table of results
(host × exit / time / output). Filterable, sortable, exportable.
The agent can `<TOOL>broadcast_query:cmd</TOOL>` and the result
arrives as one consolidated response Lucy can reason about.

**Cost:** ~3 days. **Risk:** medium — careful with concurrent
session limits per WinRM target.

### 2.4 ★★★★ **Sub-agent specialization**
**Why now:** `fork_task` exists but spawned agents are clones of the
main one. A "sub-agent specialized in PowerShell" or "in compliance
auditing" would do narrow tasks better.

**Proposal:** Pre-baked subagent personas — each is just a different
system prompt + restricted tool subset:
- **`@auditor`** — read-only, runs CIS checks, generates reports
- **`@scribe`** — converts a session into a clean runbook (markdown)
- **`@detective`** — log analysis, anomaly hunting, FTS over agent
  memories
- **`@dba`** — DB-only tools, safe queries only

Invoked from the main agent via
`<TOOL>fork_task:auditor|||audit PROD-AD-01 against CIS L1</TOOL>`.

**Cost:** ~3 days. **Risk:** low — fork machinery exists.

### 2.5 ★★★ **Voice mode v2 — dialog turns, not just dictation**
**Why now:** Lucy has STT/TTS but it's command-style:
press-and-talk, send, response. A real voice mode for incident
response would be hands-free, low-latency, and interruptible.

**Proposal:**
- VAD (voice activity detection) → no push-to-talk
- Wake word "Lucy" with a tiny local model (Picovoice/Vosk)
- Streaming TTS so speech starts before the full response is ready
- Cancel via "stop Lucy" mid-stream

**Cost:** ~5 days, plus a small native dependency. **Risk:**
medium — VAD bugs lead to either deafness or false triggers.

### 2.6 ★★★ **External event listeners — Lucy reacts to the world**
**Why now:** Lucy is purely user-driven. She could be a watchdog:
PRTG webhook fires → Lucy auto-investigates and reports. New row
in audit log of severity > 7 → Lucy starts an incident.

**Proposal:** Internal HTTP listener (loopback only, signed with
shared secret) on a configurable port. Webhook payload + a "trigger
prompt" template fires a scheduled-task-style background run.

**Cost:** ~2 days. **Risk:** medium — needs careful auth (no
remote attacker triggering free LLM calls).

---

## 3 · UX maximization (the face)

### 3.1 ★★★★ **Insights view (Hermes HUD-inspired)**
**Why now:** all of Lucy's growth signals (skills learned,
corrections made, hosts visited, principles set) are scattered.

**Proposal:** New sidebar item **Insights** with:
- "What I learned this week" (new skills + memories)
- "Most-touched hosts" (working memory aggregated)
- "Where I correct most often" (audit + retry telemetry)
- "Cost trend" (cost dashboard + sparkline)
- "Principle adherence" (a stretch — needs telemetry)

**Cost:** ~3 days. **Risk:** low — read-only view over data
that already exists.

### 3.2 ★★★★ **Replay & branch — non-destructive what-ifs**
**Why now:** today every chat is one timeline. To explore an
alternative ("what if I had NOT restarted IIS?") you start a new
tab and lose the lead-up context.

**Proposal:** Click any user/lucy turn → "Branch from here". Forks
the tab cheaply (shared prefix, divergent suffix). Both branches
visible in a tab tree. Useful for incident post-mortems and
operations rehearsal.

**Cost:** ~3 days. **Risk:** medium — UI complexity for the tree
view.

### 3.3 ★★★ **Ambient mode — Lucy on the second monitor**
**Why now:** in long ops sessions you keep Lucy in a corner waiting
for input. A passive ambient mode would surface the *most relevant*
information for whatever you're doing — current cwd, last commands,
top anomalies, principles in scope — without you asking.

**Proposal:** Frameless always-on-top window mode. No chat input,
just a live dashboard derived from working memory + dashboard
metrics. Triggered from the menu bar or a keyboard shortcut.

**Cost:** ~2 days. **Risk:** low.

### 3.4 ★★★ **Annotation overlay for runbooks**
**Why now:** Lucy can read PDFs (PDF Intelligence). She can't
*annotate* the source, leaving a gap between "I read the manual"
and "I understood it for our environment".

**Proposal:** When a `pdf_search` returns a hit, the user can
type a note that gets stored as `runbook_annotation (doc_id,
section, note, ts)`. Lucy injects relevant annotations alongside
the raw passage on subsequent searches.

**Cost:** ~2 days. **Risk:** low.

---

## 4 · Cross-cutting (architectural)

### 4.1 ★★★★★ **Embedding index promoted to primary recall**
**Why now:** episodic memories use FTS5 today. FTS is great for
exact-keyword recall but blind to paraphrase. Embeddings are
already computed in the background — they just don't dominate
retrieval yet.

**Proposal:** Two-stage retrieval:
1. Embedding cosine top-K (K=20)
2. FTS rank top-K (K=20)
3. RRF (reciprocal-rank fusion) to merge
4. Final cosine re-rank with the user's actual query

Per-query latency: ~5-10 ms on 10k memories. Recall lift: 30-50%
on paraphrased queries.

**Cost:** ~2 days. **Risk:** low — embeddings infrastructure
exists.

### 4.2 ★★★★ **Tool call streaming with progressive UI**
**Why now:** when Lucy runs a long PowerShell command, the user
sees nothing until completion. Output streams to the agent but
not to the user UI.

**Proposal:** Stream stdout/stderr line-by-line directly to the
tool card during execution. Progress bar / spinner that reflects
real activity. Cancel button that actually kills the child process.

**Cost:** ~2 days. **Risk:** low for shell.rs (already streaming
internally for NexShell), more work for execute_powershell.

### 4.3 ★★★ **Privacy-first telemetry (opt-in, local-only)**
**Why now:** the `task_event` log persists locally. We've never
analyzed it. There's a goldmine: Lucy's first-try-success rate per
tool, per host, per model. The user could see *their* signature.

**Proposal:** A monthly auto-generated report (Markdown, opt-in),
stored at `~/Lucy/reports/2026-04.md`:
- Total tasks, success rate, avg latency
- Top 5 most-used skills
- Top 3 most-failing tools (and why)
- Cost burn-down vs budget
- Improvement suggestions

100% local. No data leaves the machine.

**Cost:** ~2 days. **Risk:** low.

### 4.4 ★★★ **Plugin sandbox — third-party skills without trust**
**Why now:** the SkillsManager runs PowerShell. A community-
contributed skill can do anything. Trust gradient is binary:
install or don't.

**Proposal:** Skills declare their *capabilities* (e.g.
`reads:files`, `writes:registry`, `network:outbound`) and run in
a permission-checked wrapper that consults Permission Rules
before executing. New skills propose their cap set; user approves
or denies. Mature pattern (extension stores).

**Cost:** ~5 days. **Risk:** medium — defining the cap taxonomy
takes care.

---

## 5 · Speculative (research-grade)

### 5.1 **In-context fine-tuning on user style**
After 100+ turns Lucy has enough samples of *how the user thinks*
to bias her own outputs toward that style. Implementable via a
"style digest" extracted by a background mini-LLM and injected
into CORE memory.

### 5.2 **Active learning for skill triggers**
Currently Lucy matches skill triggers via substring. A small
local embedding model could match "when I say 'restart IIS' or
anything similar". Lucy nudges the user to confirm low-confidence
matches, learning over time.

### 5.3 **Differential audit — what would have failed yesterday?**
Combine inventory diff (2.2) + agent memories. Lucy can answer
"if today's vulnerabilities had existed last week, which of my
hosts would have been exposed?". Useful for compliance audits
and post-incident timelines.

### 5.4 **Lucy-to-Lucy federation**
Two SysAdmins running Lucy on separate networks could share
*signed memory bundles* (the `.lucymem` format from RESEARCH.md
section 4.3). One discovers a fix, the other inherits it without
ever sharing prompts. Privacy-preserving collective learning.

### 5.5 **Sandbox-by-default execution**
Default to dry-run for any destructive command. The user has to
explicitly opt-in to *execute for real*. A muscle-memory fork of
the current PLAN/VERIFY system but with the safe path pre-selected.

---

## 6 · Quick-wins (≤4 hours each)

These deliver disproportionate value for tiny effort. Land them
between bigger features:

| Idea | Impact | Effort |
|---|---|---|
| Ctrl+/ to expand last `<THOUGHT>` block | Debugging UX | 1h |
| Pin a memory or principle to the top of every prompt | Power users | 1h |
| `/cost` slash command shows prediction + monthly burn | Visibility | 1h |
| Drag a `.lucynote` file → restore session | Continuity | 2h |
| Auto-compact tabs idle > 30 min | Memory hygiene | 2h |
| Color-code the agent reasoning bubble by confidence level | Trust | 1h |
| Right-click any path in chat → open in NexShell at that cwd | Flow | 2h |
| `Ctrl+Shift+R` rerun last user prompt unchanged | Iteration speed | 1h |
| Surface anomaly badge as system tray icon when window minimized | Background ops | 3h |
| Skills export → `.zip` for sharing or backup | Portability | 2h |

---

## 7 · How I'd sequence this if I were the PM

**Sprint 1 (1 week)** — high-leverage cognition
1. 1.3 Intent classifier (cheap, multiplies everything else)
2. 2.1 Cost-aware routing (cost cut, immediate)
3. 4.1 Embedding-first recall (memory quality jump)

**Sprint 2 (1 week)** — make Lucy useful as a watchdog
4. 1.1 Plan-then-Execute multi-step
5. 2.2 Inventory diff (ride scheduled tasks)
6. 2.6 External event listener

**Sprint 3 (1 week)** — visible, measurable progress for the user
7. 3.1 Insights view
8. 4.3 Local-only monthly reports
9. 1.6 `/why` explainability + 5 quick-wins

After three sprints Lucy goes from "great solo assistant" to
"team-member-grade ops partner". Everything proposed here builds on
existing infrastructure — none of it requires a rewrite.
