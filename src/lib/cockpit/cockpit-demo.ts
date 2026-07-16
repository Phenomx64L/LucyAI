/* ============================================================================
   Lucy 2.0 — Cockpit demo driver  ·  Phase F3 (UI 2.0, direction C)
   ----------------------------------------------------------------------------
   Plays a realistic scripted agent run ("diagnose + fix a downed Ethernet
   adapter on PROD-LINUX") into the agent-workspace store, so the cockpit panels
   are ALIVE and demoable at /cockpit before the real +page.svelte bridge exists.
   This file ships ONLY the /cockpit preview route — it is not part of the
   production agent path. Timings use setTimeout so the operator watches the
   plan advance step by step. Returns a cancel fn that clears the timeline.
   ========================================================================== */
import {
  resetWorkspace, agentPlan, planUpdate, execPush, tracePush, artifactPush,
  statusPatch, type PlanStep,
} from './agent-workspace';

export function runCockpitDemo(): () => void {
  const timers: number[] = [];
  const at = (ms: number, fn: () => void): void => {
    timers.push(setTimeout(fn, ms) as unknown as number);
  };

  resetWorkspace();

  const steps: PlanStep[] = [
    { id: 's1', label: 'Inspeccionar estado del adaptador',    status: 'pending', detail: 'Get-NetAdapter',            host: 'PROD-LINUX' },
    { id: 's2', label: 'Revisar driver e2xw y eventos',        status: 'pending', detail: 'Get-WinEvent',             host: 'PROD-LINUX' },
    { id: 's3', label: 'Reactivar interfaz Ethernet',          status: 'pending', detail: 'Restart-NetAdapter',       host: 'PROD-LINUX' },
    { id: 's4', label: 'Verificar conectividad',               status: 'pending', detail: 'Test-Connection 8.8.8.8',  host: 'PROD-LINUX' },
    { id: 's5', label: 'Guardar hallazgo en memoria del host', status: 'pending',                                     host: 'PROD-LINUX' },
  ];
  agentPlan.set(steps);
  statusPatch({ running: true, stepIndex: 0, stepTotal: steps.length, host: 'PROD-LINUX', model: 'Opus 4.8', costUsd: 0 });

  // ── Step 1 · inspect ──
  at(300,  () => { planUpdate('s1', { status: 'running' }); statusPatch({ stepIndex: 1 }); tracePush({ phase: 'think', label: 'Enumerar adaptadores y su estado', step: 1 }); });
  at(1100, () => {
    execPush({ engine: 'PS', ok: true, ms: 410, cmd: 'Get-NetAdapter | Format-Table Name,Status,LinkSpeed',
      output: 'Name       Status  LinkSpeed\nWi-Fi      Up      866.7 Mbps\nvEthernet  Up      10 Gbps\nEthernet   Down    0 bps' });
    planUpdate('s1', { status: 'done', ms: 410 });
    tracePush({ phase: 'react', label: 'Ethernet está Down — sigo con el driver', step: 1 });
    statusPatch({ costUsd: 0.08 });
  });

  // ── Step 2 · driver + events ──
  at(1300, () => { planUpdate('s2', { status: 'running' }); statusPatch({ stepIndex: 2 }); tracePush({ phase: 'act', label: 'Buscar errores del driver e2xw', step: 2 }); });
  at(2200, () => {
    execPush({ engine: 'PS', ok: true, ms: 520, cmd: 'Get-WinEvent -LogName System -MaxEvents 5 | Where Id -eq 4103',
      output: 'e2xw: la solicitud del adaptador expiró (timeout). El controlador dejó de responder.' });
    planUpdate('s2', { status: 'done', ms: 520 });
    tracePush({ phase: 'react', label: 'Driver colgado — un reinicio del adaptador debería bastar', step: 2 });
    statusPatch({ costUsd: 0.17 });
  });

  // ── Step 3 · reactivate (a mutating command — HITL-gated in the real loop) ──
  at(2400, () => { planUpdate('s3', { status: 'running' }); statusPatch({ stepIndex: 3 }); tracePush({ phase: 'act', label: 'Reiniciar la interfaz Ethernet', step: 3 }); });
  at(3400, () => {
    execPush({ engine: 'PS', ok: true, ms: 2100, cmd: 'Restart-NetAdapter -Name Ethernet -Confirm:$false',
      output: 'Ethernet   Up      1 Gbps' });
    planUpdate('s3', { status: 'done', ms: 2100 });
    statusPatch({ costUsd: 0.24 });
  });

  // ── Step 4 · verify ──
  at(3600, () => { planUpdate('s4', { status: 'running' }); statusPatch({ stepIndex: 4 }); tracePush({ phase: 'act', label: 'Probar salida a internet', step: 4 }); });
  at(4500, () => {
    execPush({ engine: 'PS', ok: true, ms: 640, cmd: 'Test-Connection 8.8.8.8 -Count 2',
      output: 'Respuesta desde 8.8.8.8: tiempo=24ms\nRespuesta desde 8.8.8.8: tiempo=22ms\n0% de pérdida' });
    planUpdate('s4', { status: 'done', ms: 640 });
    tracePush({ phase: 'react', label: 'Conectividad restaurada', step: 4 });
    statusPatch({ costUsd: 0.29 });
  });

  // ── Step 5 · persist to host memory (artifact) ──
  at(4700, () => { planUpdate('s5', { status: 'running' }); statusPatch({ stepIndex: 5 }); tracePush({ phase: 'act', label: 'Guardar el patrón de reparación', step: 5 }); });
  at(5500, () => {
    artifactPush({ kind: 'memory', path: 'PROD-LINUX · memoria', summary: 'Ethernet caído por driver e2xw colgado → Restart-NetAdapter lo resuelve.' });
    planUpdate('s5', { status: 'done', ms: 180 });
    statusPatch({ running: false, costUsd: 0.31 });
    tracePush({ phase: 'info', label: 'Tarea completada · 4 comandos · 1 memoria', step: 5 });
  });

  return () => { timers.forEach((t) => clearTimeout(t)); };
}
