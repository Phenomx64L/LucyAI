---
name: generating-windows-system-health-and-security-report
description: 'Produces a structured Windows system report that synthesises performance and security signals into a single Markdown document and writes it to disk. Combines sysinfo, tasklist, network connections, recent event log entries, Defender state, autoruns, hotfix list, and disk usage into an executive summary plus per-axis sections plus actionable findings. Activates when the user asks Lucy to "generate / produce / build / write / compile / elaborate" a "report / informe / reporte / auditoría / overview" of "the machine / la maquina / mi equipo / mi laptop / the host" with optional persistence target (escritorio / desktop / archivo / .md / .pdf).

  '
domain: sysadmin
subdomain: system-reporting
tags:
- system-health-report
- security-posture
- performance-baseline
- windows-audit
- multi-signal-synthesis
- writefile-deliverable
version: 1.1.0
author: lucy-team
license: GPLv3
revisions:
  - v1.0.0 (2026-06-03, Lucy v1.7.50) initial release alongside RULE 0b.
  - v1.1.0 (2026-06-04, Lucy v1.7.66) refresh after the Mission Control
    overhaul. Adds explicit `<CITE>` syntax for evidence references in the
    Hallazgos table, instructs Lucy to preserve warp-block metadata
    (hostname / engine / timestamp / exit code) verbatim in the appendix,
    correlates the report's executive summary with the Mission Strip's
    live alerts count, surfaces the per-tab investigation tint as
    implicit context for the LLM, and adds a self-verification checklist
    Lucy walks through before emitting the final narrative.
nist_csf:
- ID.AM-02
- ID.RA-01
- DE.CM-01
- DE.CM-07
- RS.AN-01
mitre_attck:
- T1057
- T1082
- T1518
- T1518.001
nist_ai_rmf:
- MEASURE-2.7
---

# Generating a Windows System Health & Security Report

## When to Use

- The user asks for a structured assessment of a Windows machine ("genera un reporte detallado del estado de mi maquina, tanto a nivel seguridad como de rendimiento")
- The deliverable must include BOTH performance signals (CPU, memory, disk, processes) AND security signals (event log, Defender, autoruns, listening ports)
- The output is expected as a single file on disk, typically Markdown on the desktop, occasionally PDF
- The user uses generation verbs (`genera`, `produce`, `elabora`, `redacta`, `compila`, `construye`, `generate`, `build`) paired with a report noun (`reporte`, `informe`, `auditoría`, `overview`, `snapshot`)

**Do not use** for:

- Single-signal questions ("¿cuánta RAM tengo?") — those belong in Lucy's quick-tool short-circuit path
- Forensic incident investigation — use `analyzing-windows-event-logs-in-splunk` or `building-incident-timeline-with-timesketch` instead
- Continuous monitoring requirements — point the user at the Dashboard view, not a one-shot report

## Prerequisites

- Lucy is running on Windows 10/11 (the tool catalogue uses Windows-specific channels)
- Local admin is NOT required for the bulk of the report; only the Security event log channel needs elevation (handled per RULE 2 in the system prompt)
- A writable destination path (default: `%USERPROFILE%\Desktop`)
- The `<TOOL>writefile:</TOOL>` native tool is available (always true in the bundled Lucy build)

## Workflow

### Step 1: Detect the Request and Plan

When the user prompt matches the trigger phrasing, emit a single `<THOUGHT>` block FIRST that lists:

1. The data points to gather (subset of the canonical list below, selected by the user's axes)
2. The output path (resolve `%USERPROFILE%\Desktop\reporte_<hostname>_<yyyymmdd>.md` when not specified)
3. The output format (Markdown by default, PDF only if explicitly requested)

The `<THOUGHT>` block trips the agent loop in `+page.svelte` and prevents the sysinfo quick-tool short-circuit from firing. Without it, single-tool responses are dumped raw and the file is never written.

### Step 2: Gather Signals — Performance Axis

Issue each `<TOOL>` invocation in order. Wait for each result before deciding whether the next signal is still needed.

```
<TOOL>sysinfo</TOOL>
<TOOL>tasklist</TOOL>
<TOOL>netconn</TOOL>
<TOOL>eventlog:System:50:Error</TOOL>
<TOOL>eventlog:Application:50:Error</TOOL>
```

What each contributes to the performance section:

| Tool | Performance value |
|------|-------------------|
| `sysinfo` | hostname, OS, uptime, CPU model + cores, RAM total/used, disk capacity |
| `tasklist` | top processes by RAM, suspect long-running tasks, child of svchost outliers |
| `netconn` | listening ports, established outbound count, suspicious foreign IPs |
| `eventlog:System:50:Error` | recent driver / disk / power errors that explain throttling |
| `eventlog:Application:50:Error` | crashing apps, missing prerequisites |

### Step 3: Gather Signals — Security Axis

```
<TOOL>eventlog:Security:200:FailedLogin</TOOL>
<TOOL>autoruns</TOOL>            <!-- if available; otherwise: -->
<TOOL>registry:HKLM|SOFTWARE\Microsoft\Windows\CurrentVersion\Run|</TOOL>
<TOOL>registry:HKLM|SOFTWARE\Wow6432Node\Microsoft\Windows\CurrentVersion\Run|</TOOL>
<EXECUTE>Get-MpComputerStatus | ConvertTo-Json -Depth 3</EXECUTE>
<EXECUTE>Get-HotFix | Select-Object -First 20 HotFixID, Description, InstalledOn | ConvertTo-Json</EXECUTE>
<EXECUTE>Get-NetFirewallProfile | Select-Object Name, Enabled, DefaultInboundAction, DefaultOutboundAction | ConvertTo-Json</EXECUTE>
```

What each contributes:

| Tool / Command | Security value |
|----------------|----------------|
| `eventlog:Security:200:FailedLogin` | 4625 events — brute force attempts, lockouts |
| `autoruns` / `Run` keys | persistence mechanisms running at boot |
| `Get-MpComputerStatus` | Defender real-time protection state, signature freshness |
| `Get-HotFix` | last 20 patches, age of newest install vs current date |
| `Get-NetFirewallProfile` | firewall state per profile, inbound/outbound default action |

**Admin note.** `eventlog:Security:*` requires elevation. If the call returns `SECURITY_BLOCK` or empty, fall through to phase 2 per RULE 2(c): include a single copy-paste PowerShell block at the end of the report for the user to run as admin and re-attach.

### Step 4: Synthesise the Markdown Document

Compose ONE Markdown document with this structure:

```markdown
# Reporte de Estado — <HOSTNAME>

**Generado:** <YYYY-MM-DD HH:MM>
**Versión Lucy:** <LUCY_VERSION>
**Operador:** <USER_NAME>

---

## Resumen Ejecutivo

- <Top finding 1, with severity tag in [info|warn|crit]>
- <Top finding 2>
- <Top finding 3>

> Ejemplo:
> - [warn] Defender Real-Time Protection está deshabilitado.
> - [info] CPU promedio en 54%; chrome.exe consume 3.2 GB sostenidos.
> - [crit] 18 intentos de logon fallidos (EID 4625) en las últimas 24h desde 192.168.1.83.

---

## 1. Rendimiento

### 1.1 Hardware base
| Métrica | Valor |
|---------|-------|
| Modelo CPU | <from sysinfo> |
| Núcleos lógicos | <n> |
| RAM total | <X GB> |
| Almacenamiento | <type + capacity> |

### 1.2 Carga actual
- Uso global CPU: <X%>
- RAM en uso: <X MB / Y MB (Z%)>
- Procesos hambrientos: top 5 por RAM con PID + nombre

### 1.3 Eventos recientes de rendimiento
- Errores System: <n events>; eventos críticos:
  - EID <id> · <source> · <message preview>

---

## 2. Seguridad

### 2.1 Postura del antivirus
- Real-time protection: <enabled/disabled>
- Última actualización de firmas: <date>
- Última exploración: <date>

### 2.2 Persistencia
- Entradas en `HKLM\...\Run`: <n>; entradas notables:
  - <name> → <command>

### 2.3 Eventos de seguridad
- EID 4625 (logon fallido): <n eventos en últimas 24h>; orígenes:
  - <IP/user> · <count>

### 2.4 Patches
- Último hotfix instalado: <KBxxxxxx> el <date>
- Días desde el último parche: <n>

### 2.5 Firewall
- Profile Domain: <state>
- Profile Private: <state>
- Profile Public: <state>

---

## 3. Hallazgos y Recomendaciones

| Severidad | Hallazgo | Acción sugerida |
|-----------|----------|-----------------|
| [crit] | <claim> <CITE src="tool:tasklist" kind="tool">tasklist</CITE> | <one concrete command or step> |
| [warn] | <claim> <CITE src="C:\Windows\System32\winevt\Logs\Security.evtx" kind="file">EID 4625</CITE> | <action> |
| [info] | <claim> <CITE src="https://learn.microsoft.com/..." kind="url">MS docs</CITE> | <action> |

**Citation syntax (added in v1.1.0).** Every claim MUST cite the tool, file,
or URL that produced the evidence using a `<CITE>` tag. Lucy's frontend
(v1.7.63) renders these as colour-coded **evidence pills**:

| `kind` | Colour | When to use |
|---|---|---|
| `memory` | cyan | Recalled from agent_memories / memory_core |
| `file` | green | Specific file or evtx path on disk |
| `url` | blue | Web reference (vendor docs, CVE entry, etc.) |
| `tool` | amber | Output of a tool you ran in this turn (`tasklist`, `sysinfo`, etc.) |

A bare `<claim>` with no `<CITE>` is a RULE 33 violation: rephrase as
`(hypothesis)` and add a verification command if the evidence isn't yet
on the table.

---

## 4. Apéndice — Datos crudos

Every tool invocation Lucy made in this turn renders in the chat as a
"terminal-recording" warp-block (v1.7.60) with `hostname · engine glyph ·
HH:MM:SS · elapsed · exit code` baked into its header. **Preserve those
chrome fields verbatim** when you transcribe the block into the appendix —
they are forensic context, not decoration. The collapsible summary should
include the hostname so the appendix scans as a list of recordings:

<details>
<summary>PRECISION-X · ⚡ sysinfo · 14:23:01 · 142ms · exit 0</summary>

```
<raw output>
```

</details>

<details>
<summary>PRECISION-X · ⚡ tasklist (top 25) · 14:23:08 · 87ms · exit 0</summary>

```
<raw output>
```

</details>

<!-- repetir por cada tool invocado, conservando hostname + engine + ts -->
```

### Step 5: Persist the Report

Always finish the gathering+synthesis phase with a SINGLE writefile call:

```
<TOOL>writefile:<resolved-path>.md</TOOL>
<FILECONTENT>
...the complete Markdown document from Step 4...
</FILECONTENT>
```

Resolved path defaults:

- User said "escritorio" / "desktop" → `%USERPROFILE%\Desktop\reporte_<hostname>_<YYYYMMDD>.md`
- User specified a folder → that folder + the canonical filename
- No persistence target mentioned → render inline in chat (a hint that the user did NOT want a file — do NOT writefile)

### Step 6: Final Narrative (Brief)

After the writefile, end with at most 6 lines in chat:

```
Reporte generado: C:\Users\Iván\Desktop\reporte_PRECISION-X_20260603.md

Top 3:
- [crit] 18 intentos de logon fallidos desde 192.168.1.83 (últ. 24h)
- [warn] Defender Real-Time Protection deshabilitado
- [info] CPU promedio 54% durante 88h de uptime

¿Quieres que aplique el endurecimiento de Defender ahora?
```

## Chrome context Lucy can read (v1.1.0)

The Lucy UI itself carries operational context that this skill should
respect:

### Mission Strip alerts count (v1.7.58)

The top-of-window status band always shows `⚠ N alerts` derived from
`activeIncidentId`. The **count of `[crit]` rows** in the Resumen
Ejecutivo MUST equal that number at generation time. If Lucy is about to
emit a `[crit]` row but `activeAlerts === 0`, she should escalate the
new incident first (so the band reflects reality) OR downgrade the row
to `[warn]` with a verification command for the operator to confirm.
Inconsistency between the band and the report is a credibility leak.

### Per-tab investigation tint (v1.7.59)

A tab whose content matches the investigation regex (phishing, malware,
threat, breach, forensic, CVE, ransom, c2, intrusion, etc.) gets an
amber border-top automatically. **If Lucy detects the active tab is
already tinted as `investigation`,** she can skip re-asking about
context — the operator has implicitly classified this turn. Bias the
report towards Security depth: include EID 4625 origin breakdown,
autorun deltas vs. last clean snapshot, and Defender exclusion-rule
audit, even if the user's prompt didn't explicitly demand them.

### Streaming pipeline guarantees (v1.7.45-65)

Code blocks Lucy emits during the report render with Shiki
pre-highlighting (v1.7.55), morphdom in-place updates (v1.7.56), and a
"Lucy is reasoning" aura while `<THOUGHT>` blocks are open (v1.7.57).
None of this changes how Lucy writes the report — but it does mean she
can emit longer narrative blocks without worrying about flicker. There
is no streaming-performance reason to be terse.

## Self-verification checklist (v1.1.0)

Before emitting the final narrative (Step 6), walk the checklist below
silently and only commit the response if every box is checked. If any
fails, fix it before the writefile, not after.

```
□ Resumen Ejecutivo contains 3-5 bullets
□ Every bullet is severity-tagged [crit|warn|info]
□ [crit] bullet count matches Mission Strip's activeAlerts (or has
  been explicitly reconciled in this turn)
□ Every claim in Sección 3 has a <CITE src="…" kind="…"> tag
□ Every tool transcribed into Sección 4 preserves hostname + engine +
  HH:MM:SS + elapsed + exit code from its warp-block header
□ The Markdown document references its own generation timestamp
□ <TOOL>writefile:…</TOOL><FILECONTENT>…</FILECONTENT> is the LAST
  data action before the chat narrative
□ Chat narrative is ≤6 lines and contains the exact file path
□ Chat narrative offers ONE concrete follow-up (e.g. "¿aplicar el
  endurecimiento de Defender?")
```

A failing checkbox is treated like a RULE 33 violation: do not ship
the response with the gap. Either repair it in the same turn or ask
the operator a single clarifying question (RULE 31 Ambiguity Gate).

## Key Concepts

| Term | Definition |
|------|------------|
| **Report Generation Intent** | A first-class Lucy intent class (E) added in v1.7.50 that specialises RULE 0's classification. Triggers on generation verb + report noun + (optional) persistence target. |
| **Quick-Tool Short-Circuit** | The `+page.svelte:4521` fast path that runs a single native TOOL and stops. Bypassed for reports because reports need multi-signal synthesis + writefile. |
| **Completion Contract (RULE 2b)** | Lucy must DELIVER every promised artifact in the same turn. A report request is not satisfied until the file is on disk and its path is stated to the user. |
| **Phase 1 / Phase 2 elevation (RULE 2c)** | When the report needs Security event log access (admin), gather everything else in phase 1, deliver the partial Markdown, and append a phase-2 PowerShell block for the user to run elevated. |

## Tools & Systems

- **`<TOOL>sysinfo</TOOL>`** — hardware baseline, OS, uptime, RAM/disk capacity
- **`<TOOL>tasklist</TOOL>`** — top processes by memory, suspect runners
- **`<TOOL>netconn</TOOL>`** — listening ports, established connections, foreign IPs
- **`<TOOL>eventlog:<channel>:<count>[:<filter>]</TOOL>`** — System / Application / Security event log entries
- **`<TOOL>registry:HIVE|key|value</TOOL>`** — autorun keys, configuration probes
- **`<EXECUTE>Get-MpComputerStatus ...</EXECUTE>`** — Defender posture as JSON
- **`<EXECUTE>Get-HotFix ...</EXECUTE>`** — installed Windows updates
- **`<EXECUTE>Get-NetFirewallProfile ...</EXECUTE>`** — firewall profile states
- **`<TOOL>writefile:<path></TOOL>` + `<FILECONTENT>...</FILECONTENT>`** — atomic file write; ALWAYS use over `Set-Content` / `Out-File`

## Common Scenarios

### Scenario: User asks for a full report saved to desktop

**Prompt**: *"genera un reporte detallado del estado de mi maquina, tanto a nivel seguridad como de rendimiento, el reporte depositalo en mi escritorio"*

**Approach**:
1. Detect: generation verb (`genera`) + quality qualifier (`detallado`) + report noun (`reporte`) + compound axes (`tanto seguridad como rendimiento`) + persistence target (`escritorio`). Intent class E triggered.
2. Emit `<THOUGHT>` with 9 signals (5 perf + 4 security) + path resolution to `%USERPROFILE%\Desktop\reporte_<HOSTNAME>_<YYYYMMDD>.md` + format Markdown.
3. Execute each signal in sequence, waiting for each result.
4. Compose the Markdown per the Step 4 template.
5. Single `<TOOL>writefile:</TOOL>` with `<FILECONTENT>` block.
6. Final 6-line narrative with file path + top 3 findings + one follow-up offer.

**Pitfalls**:
- Emitting only `<TOOL>sysinfo</TOOL>` and finishing. The short-circuit fires and the user gets a 6-line CPU/RAM dump instead of a report. v1.7.49 + v1.7.50 prevent this but defence in depth is mandatory: always emit `<THOUGHT>` first to make the multi-step nature explicit.
- Writing the file via `Set-Content` / `Out-File` instead of `<TOOL>writefile:</TOOL>`. Native tool is faster, atomic, respects path quoting, and is observable to the user via the WriteFileChip.
- Pasting the entire 2000-line tool dump into chat. The chat narrative must be ≤6 lines after the file is written.
- Forgetting the `## Hallazgos y Recomendaciones` section. Without it the document is a transcript, not a report.

### Scenario: User asks for a security-only audit, no file

**Prompt**: *"hazme una auditoría rápida de seguridad de este equipo"*

**Approach**:
- No persistence target → render inline, no writefile.
- Single axis (Seguridad) → skip the performance signals.
- Smaller document: Resumen ejecutivo + Postura del antivirus + Persistencia + Eventos de seguridad + Hallazgos.
- 3-5 chat lines summary at the end.

### Scenario: User asks for a PDF report

**Prompt**: *"genera un reporte ejecutivo del estado y guárdalo como PDF en escritorio"*

**Approach**:
- Generate the Markdown FIRST via the Step 4 template.
- Write the Markdown to a temp `.html` (Markdown → HTML conversion done in the report itself).
- Use Edge Headless full-path per RULE for PDF generation:
  `& 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe' --headless --disable-gpu --print-to-pdf="<desktop>\reporte.pdf" "file:///<temp>\reporte.html"`
- Confirm both files exist and report the final PDF path in the chat narrative.

## Output Format

The on-disk Markdown report is the canonical deliverable. See the Step 4 template above. The chat narrative is the human-facing summary, never the report itself.
