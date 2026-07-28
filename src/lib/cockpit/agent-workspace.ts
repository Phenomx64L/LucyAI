/* ============================================================================
   Lucy 2.0 — Agent workspace store  ·  Phase F3 (UI 2.0, direction C)
   ----------------------------------------------------------------------------
   The data model behind the cockpit's right lane. The four panels
   (Plan / Ejecución / Trace / Artefactos) render PURELY from these stores, so
   whoever fills them — the demo driver today, the real +page.svelte agent loop
   at integration time — drives the same UI.

   THE INTEGRATION BRIDGE (later, behind the flag) is just a handful of calls
   from the existing agent loop into this API:
       stepsHtml step  ->  planSet(...) / planUpdate(id, { status })
       warpBlock(...)  ->  execPush({ cmd, output, ok, ms, engine })
       pushTrace(...)  ->  tracePush({ phase, label, detail, step })
       editfile/write  ->  artifactPush({ kind, path, summary })
   No new agent logic — this is re-surfacing state that already exists.
   ========================================================================== */
import { writable } from 'svelte/store';

export type StepStatus = 'pending' | 'running' | 'done' | 'error';

export interface PlanStep {
  id: string;
  label: string;
  status: StepStatus;
  detail?: string;   // the command / tool the step runs
  host?: string;     // where it runs (local host name or a remote host)
  ms?: number;       // duration once finished
  ts?: number;       // creation time (ms epoch) — for the timeline / total elapsed
}

export interface ExecEntry {
  id: string;
  cmd: string;
  output: string;
  ok: boolean;
  ms?: number;
  engine?: string;   // PS / CMD / SSH / …
  code?: number | null; // inferred exit code (null = none / success)
  ts?: number;       // creation time (ms epoch)
}

export interface TraceEntry {
  id: string;
  phase: string;     // think / act / react / info …
  label: string;
  detail?: string;
  step?: number;
  ts?: number;       // creation time (ms epoch)
}

export interface Artifact {
  id: string;
  kind: 'edit' | 'write' | 'skill' | 'memory';
  path: string;
  summary?: string;
  before?: string;  // prior file content (edit oldStr / write's read-back) — for the diff
  after?: string;   // new content (edit newStr / write content)
  ts?: number;      // creation time (ms epoch)
}

/**
 * A background sub-agent launched with `<TOOL>fork_task:…</TOOL>`.
 *
 * Forks were the one part of a turn with NO representation in the cockpit: Lucy
 * would launch two of them, carry on with the main task, and the operator saw
 * an unexplained pause. `ForksMonitorPanel` exists but only in the classic UI.
 *
 * `collected` is a distinct state from `done` on purpose. A fork finishes on its
 * own schedule, but its result only re-enters the main task when a later
 * `wait_task` picks it up — and the gap between those two moments is exactly
 * what the operator needs to see to understand why Lucy is still working.
 */
export type ForkStatus = 'running' | 'done' | 'error' | 'collected';

export interface AgentFork {
  /** The operator-facing id from the tool call — NOT a generated one, because
   *  the loop and the transcript both refer to the fork by this name. */
  id: string;
  instruction: string;
  model?: string;
  status: ForkStatus;
  result?: string;
  ms?: number;      // filled on finish, from `ts`
  ts?: number;      // launch time (ms epoch)
}

export interface AgentStatus {
  running: boolean;
  stepIndex: number;   // 1-based current step
  stepTotal: number;
  host: string | null;
  model: string | null;
  costUsd: number;
}

/**
 * One entry in the mirrored conversation. `user`/`lucy` are chat messages;
 * `thought` is a collapsible reasoning block; `tool` is an inline tool-call card
 * (both ephemeral — reflect activity during the live turn).
 */
export interface ConvoMsg {
  id: string;
  role: 'user' | 'lucy' | 'thought' | 'tool';
  text: string;
  ts?: number;       // creation time (ms epoch) — mono timestamp in the thread
  dur?: number;      // thought: seconds spent reasoning
  kind?: string;     // tool: 'exec' | 'edit' | 'write' | 'read'
  ok?: boolean;      // tool: succeeded?
  detail?: string;   // tool: output / summary (shown when expanded)
  atts?: { name: string; previewUrl?: string }[];  // user: image attachments (thumbnails)
}

export const agentPlan = writable<PlanStep[]>([]);
export const agentExec = writable<ExecEntry[]>([]);
export const agentTrace = writable<TraceEntry[]>([]);
export const agentArtifacts = writable<Artifact[]>([]);
export const agentForks = writable<AgentFork[]>([]);
export const agentStatus = writable<AgentStatus>({
  running: false, stepIndex: 0, stepTotal: 0, host: null, model: null, costUsd: 0,
});

// Conversation is CONTINUOUS across runs, so it lives outside resetWorkspace().
export const agentConvo = writable<ConvoMsg[]>([]);

let _seq = 0;
const nid = (): string => `w${++_seq}`;

/** Clear every workspace lane — call at the start of a fresh agent task. */
export function resetWorkspace(): void {
  agentPlan.set([]);
  agentExec.set([]);
  agentTrace.set([]);
  agentArtifacts.set([]);
  agentForks.set([]);
  agentStatus.set({ running: false, stepIndex: 0, stepTotal: 0, host: null, model: null, costUsd: 0 });
}

/** Register a fork the moment it is launched, so it is visible while it runs. */
export function forkStart(f: Omit<AgentFork, 'status' | 'ts' | 'ms'>): void {
  agentForks.update((l) => {
    // Re-launching a known id replaces the old row rather than duplicating it —
    // the loop already refuses duplicate ids, so a repeat here means a retry.
    const rest = l.filter((x) => x.id !== f.id);
    return [...rest, { ...f, status: 'running' as ForkStatus, ts: Date.now() }];
  });
}

/**
 * Settle a fork. Duration is derived from the stored launch time rather than
 * taken from the caller, so every row measures the same thing.
 */
export function forkFinish(id: string, patch: { status: 'done' | 'error'; result?: string }): void {
  agentForks.update((l) =>
    l.map((f) => (f.id === id ? { ...f, ...patch, ms: f.ts ? Date.now() - f.ts : undefined } : f)),
  );
}

/** Mark a finished fork as folded back into the main task by `wait_task`. */
export function forkCollected(id: string): void {
  agentForks.update((l) => l.map((f) => (f.id === id ? { ...f, status: 'collected' } : f)));
}

/** Replace the whole plan (e.g. once the loop has decomposed the task). */
export function planSet(steps: PlanStep[]): void {
  agentPlan.set(steps);
}

/** Patch a single step by id (mark running/done/error, set duration, …). */
export function planUpdate(id: string, patch: Partial<PlanStep>): void {
  agentPlan.update((list) => list.map((s) => (s.id === id ? { ...s, ...patch } : s)));
}

/** Append one step (auto-id). Used when steps arrive incrementally. */
export function planAppend(step: Omit<PlanStep, 'id'>): void {
  agentPlan.update((l) => [...l, { id: nid(), ...step, ts: step.ts ?? Date.now() }]);
}

export function execPush(entry: Omit<ExecEntry, 'id'>): void {
  agentExec.update((l) => [...l, { id: nid(), ...entry, ts: entry.ts ?? Date.now() }]);
}

export function tracePush(entry: Omit<TraceEntry, 'id'>): void {
  agentTrace.update((l) => [...l, { id: nid(), ...entry, ts: entry.ts ?? Date.now() }]);
}

export function artifactPush(a: Omit<Artifact, 'id'>): void {
  // Bound the stored before/after so a big writefile can't bloat the store, and
  // keep only the last 60 artifacts (a long session can touch many files).
  const CAP = 8000;
  const clip = (s?: string) => (s == null ? undefined : s.length > CAP ? s.slice(0, CAP) + '\n… (truncado)' : s);
  const rec: Artifact = { id: nid(), ...a, before: clip(a.before), after: clip(a.after), ts: a.ts ?? Date.now() };
  agentArtifacts.update((l) => [...l, rec].slice(-60));
}

export function statusPatch(patch: Partial<AgentStatus>): void {
  agentStatus.update((s) => ({ ...s, ...patch }));
}

/** Mirror a conversation line into the cockpit (bounded to 200). */
export function convoPush(m: Omit<ConvoMsg, 'id'>): void {
  agentConvo.update((l) => {
    const next = l.length >= 200 ? l.slice(-199) : l.slice();
    // v1.7.236 iter-2 — stamp creation time so the thread renders a mono
    // timestamp per message (an explicit ts from the caller still wins).
    next.push({ id: nid(), ts: Date.now(), ...m });
    return next;
  });
}

export function convoReset(): void { agentConvo.set([]); }

/** Live, in-progress Lucy reply for the active tab — updated token-by-token
 *  while streaming, cleared when the turn settles (the final line then lives in
 *  agentConvo). Renders as a trailing "typing" bubble under the conversation. */
export const agentStream = writable<string>('');
export function streamSet(text: string): void { agentStream.set(text); }
export function streamClear(): void { agentStream.set(''); }
