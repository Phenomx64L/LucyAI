<!-- ── EnrichedOutputWidget.svelte — Rich visualization for structured command output ──
     Receives an EnrichedOutput object and renders the appropriate widget:
     - process-table: sortable process list with CPU/mem columns
     - service-list: status-colored service rows
     - disk-usage: capacity bars with percentages
     - network-table: connection state table
     - json-tree: collapsible JSON viewer
     - key-value: aligned key-value grid
     - plain: passthrough (not rendered here)
-->
<script lang="ts">
    import type { EnrichedOutput, ProcessRow, ServiceRow, DiskRow, NetRow, KVPair, EventLogRow } from '$lib/output-enricher';

    export let data: EnrichedOutput;
    export let collapsed: boolean = false;

    let sortCol: string = '';
    let sortAsc: boolean = true;
    let exportMsg: string = '';

    function toggleSort(col: string) {
        if (sortCol === col) { sortAsc = !sortAsc; }
        else { sortCol = col; sortAsc = true; }
    }

    // Reactive sorted process rows
    $: procRows = data.type === 'process-table' ? sortProcesses(data.parsed, sortCol, sortAsc) : [];
    $: serviceRows = data.type === 'service-list' ? (data.parsed as ServiceRow[]) : [];
    $: diskRows = data.type === 'disk-usage' ? (data.parsed as DiskRow[]) : [];
    $: netRows = data.type === 'network-table' ? (data.parsed as NetRow[]) : [];
    $: kvPairs = data.type === 'key-value' ? (data.parsed as KVPair[]) : [];
    $: eventRows = data.type === 'event-log' ? (data.parsed as EventLogRow[]) : [];

    // ── CSV / clipboard export ─────────────────────────────────────────
    function tableToCsv(headers: string[], rows: any[][]): string {
        const esc = (v: any) => {
            let s = String(v ?? '');
            // CSV injection defense: prefix formula-triggering chars
            if (/^[=+\-@\t\r]/.test(s)) s = "'" + s;
            return s.includes(',') || s.includes('"') || s.includes('\n')
                ? `"${s.replace(/"/g, '""')}"` : s;
        };
        return [headers.map(esc).join(','), ...rows.map(r => r.map(esc).join(','))].join('\n');
    }

    function getExportData(): { csv: string; filename: string } | null {
        switch (data.type) {
            case 'process-table':
                return { csv: tableToCsv(['Name','PID','CPU','Mem'], procRows.map(r => [r.name, r.pid, r.cpu, r.mem])), filename: 'processes.csv' };
            case 'service-list':
                return { csv: tableToCsv(['Status','Name','DisplayName'], serviceRows.map(r => [r.status, r.name, r.displayName])), filename: 'services.csv' };
            case 'disk-usage':
                return { csv: tableToCsv(['Drive','UsedGB','FreeGB','TotalGB','UsedPct'], diskRows.map(r => [r.drive, r.usedGB, r.freeGB, r.totalGB, r.usedPct])), filename: 'disks.csv' };
            case 'network-table':
                return { csv: tableToCsv(['Proto','LocalAddr','LocalPort','RemoteAddr','RemotePort','State','PID'], netRows.map(r => [r.protocol, r.localAddr, r.localPort, r.remoteAddr, r.remotePort, r.state, r.pid ?? ''])), filename: 'connections.csv' };
            case 'key-value':
                return { csv: tableToCsv(['Key','Value'], kvPairs.map(r => [r.key, r.value])), filename: 'keyvalue.csv' };
            case 'event-log':
                return { csv: tableToCsv(['Level','Time','Source','ID','Message'], eventRows.map(r => [r.level, r.time, r.source, r.id, r.message])), filename: 'events.csv' };
            default: return null;
        }
    }

    async function copyToClipboard() {
        const d = getExportData();
        if (!d) return;
        try {
            await navigator.clipboard.writeText(d.csv);
            exportMsg = 'Copied!';
        } catch { exportMsg = 'Copy failed'; }
        setTimeout(() => exportMsg = '', 2000);
    }

    function downloadCsv() {
        const d = getExportData();
        if (!d) return;
        const blob = new Blob([d.csv], { type: 'text/csv;charset=utf-8;' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url; a.download = d.filename; a.click();
        URL.revokeObjectURL(url);
        exportMsg = 'Downloaded!';
        setTimeout(() => exportMsg = '', 2000);
    }

    function sortProcesses(rows: ProcessRow[], col: string, asc: boolean): ProcessRow[] {
        if (!rows || !col) return rows || [];
        const sorted = [...rows].sort((a, b) => {
            const va = (a as any)[col];
            const vb = (b as any)[col];
            if (typeof va === 'number' && typeof vb === 'number') return va - vb;
            return String(va).localeCompare(String(vb));
        });
        return asc ? sorted : sorted.reverse();
    }

    function statusColor(status: string): string {
        const s = (status || '').toLowerCase();
        if (s === 'running' || s === 'established') return 'var(--ops-status-ok)';
        if (s === 'stopped' || s === 'close_wait' || s === 'time_wait') return 'var(--ops-status-error)';
        if (s === 'paused' || s === 'listen' || s === 'syn_sent') return 'var(--ops-status-warn)';
        return 'var(--ops-status-muted)';
    }

    // JSON tree state
    let jsonExpanded: Set<string> = new Set(['$']);
    function toggleJson(path: string) {
        if (jsonExpanded.has(path)) jsonExpanded.delete(path);
        else jsonExpanded.add(path);
        jsonExpanded = new Set(jsonExpanded); // trigger reactivity
    }
</script>

{#if !collapsed}
<div class="ops-widget ops-type-{data.type}">

    <!-- Export toolbar -->
    {#if data.type !== 'json-tree' && data.type !== 'plain'}
    <div class="ops-export-bar">
        <button class="ops-export-btn" on:click={copyToClipboard} title="Copy as CSV">📋 Copy</button>
        <button class="ops-export-btn" on:click={downloadCsv} title="Download CSV">⬇ CSV</button>
        {#if exportMsg}<span class="ops-export-msg">{exportMsg}</span>{/if}
    </div>
    {/if}

    {#if data.type === 'process-table'}
    <div class="ops-table-wrap">
        <table class="ops-table">
            <thead>
                <tr>
                    <th class="sortable" on:click={() => toggleSort('name')}>
                        Name {sortCol === 'name' ? (sortAsc ? '▲' : '���') : ''}
                    </th>
                    <th class="sortable num" on:click={() => toggleSort('pid')}>
                        PID {sortCol === 'pid' ? (sortAsc ? '▲' : '▼') : ''}
                    </th>
                    <th class="sortable num" on:click={() => toggleSort('cpu')}>
                        CPU {sortCol === 'cpu' ? (sortAsc ? '▲' : '▼') : ''}
                    </th>
                    <th class="num">Mem</th>
                </tr>
            </thead>
            <tbody>
                {#each procRows.slice(0, 50) as row}
                <tr>
                    <td class="name">{row.name}</td>
                    <td class="num pid">{row.pid}</td>
                    <td class="num cpu">{row.cpu > 0 ? row.cpu.toFixed(1) + '%' : '-'}</td>
                    <td class="num mem">{row.mem || '-'}</td>
                </tr>
                {/each}
            </tbody>
        </table>
        {#if procRows.length > 50}
        <div class="ops-overflow">+{procRows.length - 50} more rows</div>
        {/if}
    </div>

    {:else if data.type === 'service-list'}
    <div class="ops-table-wrap">
        <table class="ops-table">
            <thead>
                <tr>
                    <th>Status</th>
                    <th>Name</th>
                    <th>Display Name</th>
                </tr>
            </thead>
            <tbody>
                {#each serviceRows as svc}
                <tr>
                    <td>
                        <span class="ops-status-dot" style="color:{statusColor(svc.status)}">&bull;</span>
                        {svc.status}
                    </td>
                    <td class="name">{svc.name}</td>
                    <td class="display-name">{svc.displayName}</td>
                </tr>
                {/each}
            </tbody>
        </table>
    </div>

    {:else if data.type === 'disk-usage'}
    <div class="ops-disk-grid">
        {#each diskRows as disk}
        <div class="ops-disk-row">
            <span class="disk-label">{disk.drive}</span>
            <div class="disk-bar-track">
                <div class="disk-bar-fill"
                     style="width:{disk.usedPct}%; background:{disk.usedPct > 90 ? 'var(--ops-status-error)' : disk.usedPct > 75 ? 'var(--ops-status-warn)' : 'var(--ops-disk)'}"
                ></div>
            </div>
            <span class="disk-pct">{disk.usedPct}%</span>
            <span class="disk-detail">{disk.usedGB.toFixed(1)}G / {disk.totalGB.toFixed(1)}G</span>
        </div>
        {/each}
    </div>

    {:else if data.type === 'network-table'}
    <div class="ops-table-wrap">
        <table class="ops-table ops-table-compact">
            <thead>
                <tr>
                    <th>Proto</th>
                    <th>Local</th>
                    <th>Remote</th>
                    <th>State</th>
                    <th class="num">PID</th>
                </tr>
            </thead>
            <tbody>
                {#each netRows.slice(0, 80) as conn}
                <tr>
                    <td class="proto">{conn.protocol}</td>
                    <td class="addr">{conn.localAddr}:{conn.localPort}</td>
                    <td class="addr">{conn.remoteAddr}:{conn.remotePort}</td>
                    <td>
                        <span class="ops-state" style="color:{statusColor(conn.state)}">{conn.state}</span>
                    </td>
                    <td class="num pid">{conn.pid ?? '-'}</td>
                </tr>
                {/each}
            </tbody>
        </table>
        {#if netRows.length > 80}
        <div class="ops-overflow">+{netRows.length - 80} more connections</div>
        {/if}
    </div>

    {:else if data.type === 'json-tree'}
    <div class="ops-json">
        {@html renderJsonHtml(data.parsed, '$', 0)}
    </div>

    {:else if data.type === 'key-value'}
    <div class="ops-kv-grid">
        {#each kvPairs as pair}
        <div class="ops-kv-row">
            <span class="kv-key">{pair.key}</span>
            <span class="kv-val">{pair.value}</span>
        </div>
        {/each}
    </div>

    {:else if data.type === 'event-log'}
    <div class="ops-table-wrap">
        <table class="ops-table ops-table-compact">
            <thead>
                <tr>
                    <th>Level</th>
                    <th>Time</th>
                    <th>Source</th>
                    <th class="num">ID</th>
                    <th>Message</th>
                </tr>
            </thead>
            <tbody>
                {#each eventRows.slice(0, 100) as ev}
                <tr class="ev-row ev-{ev.level.toLowerCase()}">
                    <td>
                        <span class="ev-level ev-level-{ev.level.toLowerCase()}">{ev.level}</span>
                    </td>
                    <td class="ev-time">{ev.time}</td>
                    <td class="ev-source">{ev.source}</td>
                    <td class="num">{ev.id || '-'}</td>
                    <td class="ev-msg" title={ev.message}>{ev.message}</td>
                </tr>
                {/each}
            </tbody>
        </table>
        {#if eventRows.length > 100}
        <div class="ops-overflow">+{eventRows.length - 100} more events</div>
        {/if}
    </div>
    {/if}

</div>
{/if}

<script lang="ts" context="module">
    /** Render JSON as collapsible HTML (static — no reactivity needed inside) */
    function renderJsonHtml(obj: any, path: string, depth: number): string {
        if (depth > 6) return `<span class="json-ellipsis">...</span>`;
        if (obj === null) return `<span class="json-null">null</span>`;
        if (typeof obj === 'boolean') return `<span class="json-bool">${obj}</span>`;
        if (typeof obj === 'number') return `<span class="json-num">${obj}</span>`;
        if (typeof obj === 'string') {
            const escaped = obj.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
            return `<span class="json-str">"${escaped.length > 120 ? escaped.slice(0,120) + '…' : escaped}"</span>`;
        }
        if (Array.isArray(obj)) {
            if (obj.length === 0) return `<span class="json-bracket">[]</span>`;
            const items = obj.slice(0, 20).map((v, i) =>
                `<div class="json-item" style="padding-left:${(depth+1)*12}px">${renderJsonHtml(v, path + `[${i}]`, depth+1)}${i < obj.length - 1 ? ',' : ''}</div>`
            ).join('');
            const more = obj.length > 20 ? `<div class="json-item json-ellipsis" style="padding-left:${(depth+1)*12}px">...+${obj.length-20} items</div>` : '';
            return `<span class="json-bracket">[</span>${items}${more}<span class="json-bracket">]</span>`;
        }
        if (typeof obj === 'object') {
            const keys = Object.keys(obj);
            if (keys.length === 0) return `<span class="json-bracket">{}</span>`;
            const entries = keys.slice(0, 30).map((k, i) => {
                // Escape full HTML special set; defensive parity with the string-value branch above.
                const escaped = k.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
                return `<div class="json-item" style="padding-left:${(depth+1)*12}px"><span class="json-key">"${escaped}"</span>: ${renderJsonHtml(obj[k], path+'.'+k, depth+1)}${i < keys.length - 1 ? ',' : ''}</div>`;
            }).join('');
            const more = keys.length > 30 ? `<div class="json-item json-ellipsis" style="padding-left:${(depth+1)*12}px">...+${keys.length-30} keys</div>` : '';
            return `<span class="json-bracket">{</span>${entries}${more}<span class="json-bracket">}</span>`;
        }
        return String(obj);
    }
</script>

<style>
    .ops-widget {
        font-family: var(--ops-font, monospace);
        font-size: var(--ops-font-size-sm, 11px);
        background: var(--ops-surface-1, #0c0f1a);
        border: 1px solid var(--ops-border, rgba(255,255,255,0.06));
        border-radius: var(--ops-radius-lg, 6px);
        padding: var(--ops-space-4, 8px);
        margin: 6px 0;
        overflow: hidden;
    }

    /* ── Tables ── */
    .ops-table-wrap { overflow-x: auto; }
    .ops-table {
        width: 100%;
        border-collapse: collapse;
        font-size: var(--ops-font-size-sm, 11px);
    }
    .ops-table th {
        text-align: left;
        padding: var(--ops-space-2, 4px) var(--ops-cell-px, 8px);
        border-bottom: 1px solid var(--ops-border, rgba(255,255,255,0.06));
        color: var(--txt2, #94a3b8);
        font-weight: 500;
        height: var(--ops-header-h, 28px);
        white-space: nowrap;
        user-select: none;
    }
    .ops-table th.sortable { cursor: pointer; }
    .ops-table th.sortable:hover { color: var(--ops-status-info, #60a5fa); }
    .ops-table td {
        padding: var(--ops-space-1, 2px) var(--ops-cell-px, 8px);
        border-bottom: 1px solid rgba(255,255,255,0.02);
        height: var(--ops-row-h, 24px);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 220px;
    }
    .ops-table tbody tr:hover { background: rgba(255,255,255,0.02); }
    .ops-table .num { text-align: right; font-variant-numeric: tabular-nums; }
    .ops-table .pid { color: var(--ops-pid, #94a3b8); }
    .ops-table .cpu { color: var(--ops-cpu, #a78bfa); }
    .ops-table .mem { color: var(--ops-mem, #60a5fa); }
    .ops-table .name { font-weight: 500; }
    .ops-table .addr { font-size: 10px; color: var(--txt2, #94a3b8); }
    .ops-table .proto { font-weight: 600; color: var(--ops-net, #34d399); }
    .ops-table-compact td { padding: 1px var(--ops-cell-px, 8px); height: 20px; }
    .ops-status-dot { font-size: 14px; margin-right: 4px; }
    .ops-state { font-weight: 500; font-size: 10px; }
    .ops-overflow {
        text-align: center;
        padding: 4px;
        color: var(--txt3, #475569);
        font-size: 10px;
    }

    /* ── Disk usage ── */
    .ops-disk-grid { display: flex; flex-direction: column; gap: var(--ops-space-3, 6px); }
    .ops-disk-row {
        display: grid;
        grid-template-columns: 60px 1fr 40px 90px;
        align-items: center;
        gap: var(--ops-space-3, 6px);
    }
    .disk-label { font-weight: 600; color: var(--ops-disk, #f59e0b); }
    .disk-bar-track {
        height: 10px;
        background: var(--ops-surface-2, #111422);
        border-radius: 3px;
        overflow: hidden;
    }
    .disk-bar-fill {
        height: 100%;
        border-radius: 3px;
        transition: width var(--ops-transition, 120ms ease-out);
    }
    .disk-pct { text-align: right; font-weight: 600; font-variant-numeric: tabular-nums; }
    .disk-detail { color: var(--txt3, #475569); font-size: 10px; }

    /* ── Key-value ── */
    .ops-kv-grid { display: flex; flex-direction: column; gap: 2px; }
    .ops-kv-row {
        display: grid;
        grid-template-columns: minmax(100px, 200px) 1fr;
        gap: var(--ops-space-4, 8px);
        padding: 2px 0;
        border-bottom: 1px solid rgba(255,255,255,0.02);
    }
    .kv-key { color: var(--ops-status-info, #60a5fa); font-weight: 500; }
    .kv-val { color: var(--txt1, #e2e8f0); word-break: break-all; }

    /* ── JSON tree ── */
    .ops-json {
        font-size: var(--ops-font-size-sm, 11px);
        line-height: 1.5;
        overflow-x: auto;
        max-height: 400px;
        overflow-y: auto;
    }
    :global(.json-bracket) { color: var(--txt3, #475569); }
    :global(.json-key) { color: var(--ops-status-info, #60a5fa); }
    :global(.json-str) { color: var(--ops-status-ok, #10b981); }
    :global(.json-num) { color: var(--ops-cpu, #a78bfa); }
    :global(.json-bool) { color: var(--ops-status-warn, #f59e0b); }
    :global(.json-null) { color: var(--ops-status-muted, #475569); font-style: italic; }
    :global(.json-ellipsis) { color: var(--txt3, #475569); font-style: italic; }
    :global(.json-item) { min-height: 18px; }

    /* ── Event log ── */
    .ev-level {
        display: inline-block;
        padding: 1px 5px;
        border-radius: 3px;
        font-size: 9px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: .3px;
    }
    .ev-level-error, .ev-level-critical { background: rgba(239,68,68,.15); color: var(--ops-status-error, #ef4444); }
    .ev-level-warning { background: rgba(245,158,11,.12); color: var(--ops-status-warn, #f59e0b); }
    .ev-level-information { background: rgba(96,165,250,.10); color: var(--ops-status-info, #60a5fa); }
    .ev-time { font-size: 10px; color: var(--txt2, #94a3b8); white-space: nowrap; }
    .ev-source { font-weight: 500; font-size: 10px; }
    .ev-msg {
        max-width: 320px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 10px;
    }
    .ev-row.ev-error td, .ev-row.ev-critical td { background: rgba(239,68,68,.04); }
    .ev-row.ev-warning td { background: rgba(245,158,11,.03); }

    /* ── Export toolbar ── */
    .ops-export-bar {
        display: flex;
        align-items: center;
        gap: 6px;
        margin-bottom: 6px;
        justify-content: flex-end;
    }
    .ops-export-btn {
        background: var(--ops-surface-2, #111422);
        border: 1px solid var(--ops-border, rgba(255,255,255,0.06));
        border-radius: 4px;
        color: var(--txt2, #94a3b8);
        font-size: 10px;
        padding: 2px 8px;
        cursor: pointer;
        transition: background .15s, color .15s;
    }
    .ops-export-btn:hover {
        background: var(--ops-border, rgba(255,255,255,0.06));
        color: var(--txt1, #e2e8f0);
    }
    .ops-export-msg {
        font-size: 10px;
        color: var(--ops-status-ok, #10b981);
        animation: ops-fade 2s ease-out forwards;
    }
    @keyframes ops-fade { 0%{opacity:1;} 80%{opacity:1;} 100%{opacity:0;} }
</style>
