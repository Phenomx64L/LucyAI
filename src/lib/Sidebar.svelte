<script lang="ts">
    // v1.5.4 — full sidebar layout extracted to a single global stylesheet
    // to close the long-tail dedup loop. NexShellView / DashboardView
    // pattern reused: Sidebar.svelte's scoped style retains its
    // :global(...) refinements that override at runtime via load order.
    import '$lib/styles/sidebar.css';
    import { createEventDispatcher } from 'svelte';
    import LayoutDashboard from '@tabler/icons-svelte/icons/layout-dashboard';

    import Sparkles from '@tabler/icons-svelte/icons/sparkles';

    import TerminalSquare from '@tabler/icons-svelte/icons/terminal-2';

    import ScrollText from '@tabler/icons-svelte/icons/file-text';

    import Network from '@tabler/icons-svelte/icons/network';

    import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';

    import ClipboardList from '@tabler/icons-svelte/icons/clipboard-list';

    import Brain from '@tabler/icons-svelte/icons/brain';
    // v1.7.27 — Memory feed widget (recent memories ticker).
    import MemoryFeed from '$lib/MemoryFeed.svelte';

    import TrendingUp from '@tabler/icons-svelte/icons/trending-up';

    import Stethoscope from '@tabler/icons-svelte/icons/stethoscope';

    import Zap from '@tabler/icons-svelte/icons/bolt';

    import Download from '@tabler/icons-svelte/icons/download';

    import GraduationCap from '@tabler/icons-svelte/icons/school';

    import FileCode from '@tabler/icons-svelte/icons/file-code';

    import Settings from '@tabler/icons-svelte/icons/settings';

    import Tag from '@tabler/icons-svelte/icons/tag';

    import Bell from '@tabler/icons-svelte/icons/bell';

    import FilePdf from '@tabler/icons-svelte/icons/file-type-pdf';
    import { runbooks } from '$lib/stores';
    import { safeGetLS, safeSetLSString } from '$lib/safe-ls';
    import ActivityFeedWidget from '$lib/ActivityFeedWidget.svelte';

    // ── Props ────────────────────────────────────────────────────────────────
    export let activeView: string = 'terminal';
    export let sidebarCollapsed: boolean = false;
    export let sidebarWidth: number = 152;  // v1.5.6 — was 210
    export let sidebarResizing: boolean = false;
    export let quickActions: any[] = [];
    export let isEN: boolean = false;
    export let rshellSessions: any[] = [];
    export let registrosOpen: boolean = false;
    export let customCmdCount: number = 0;
    export let auditAlerts: number = 0;
    export let runbookRunning: any = null;
    export let showForksMonitor: boolean = false;
    export let showPdfPanel: boolean = false;
    export let ICON_MAP: Record<string, any> = {};

    // ── UI-2 (Sprint 1) — Sidebar section collapse state ─────────────────────
    // Sistema / Runbooks / Acciones directas are now collapsible (Registros
    // already was). Persisted per-section to localStorage so the user's
    // preferred layout survives reload. Default to expanded for discoverability.
    // When sidebarCollapsed === true (sidebar collapsed to icons-only), these
    // booleans are ignored — every section renders as a single column of icons.
    let sistemaOpen: boolean    = safeGetLS('lucy_sb_sistema_open', '1') !== '0';
    let runbooksOpen: boolean   = safeGetLS('lucy_sb_runbooks_open', '1') !== '0';
    let accionesOpen: boolean   = safeGetLS('lucy_sb_acciones_open', '1') !== '0';
    function toggleSection(name: 'sistema' | 'runbooks' | 'acciones') {
        if (name === 'sistema')  { sistemaOpen  = !sistemaOpen;  safeSetLSString('lucy_sb_sistema_open',  sistemaOpen  ? '1' : '0'); }
        if (name === 'runbooks') { runbooksOpen = !runbooksOpen; safeSetLSString('lucy_sb_runbooks_open', runbooksOpen ? '1' : '0'); }
        if (name === 'acciones') { accionesOpen = !accionesOpen; safeSetLSString('lucy_sb_acciones_open', accionesOpen ? '1' : '0'); }
    }

    const dispatch = createEventDispatcher<{
        setview: { view: string };
        newtab: void;
        limpiar: void;
        openmodal: { modal: string };
        runaction: { action: any };
        runrunbook: { runbook: any };
        editrunbook: { runbook: any };
        deleterunbook: { id: string };
        editaction: { index: number };
        deleteaction: { index: number };
        sbresizestart: { event: MouseEvent };
        toggleregistros: void;
        memoriaabierta: void;
        auditabierto: void;
        exportarlog: void;
        toggleforks: void;
        togglepdf: void;
    }>();
</script>

<aside class="sidebar sidebar-glass"
    class:open={!sidebarCollapsed}
    class:closed={sidebarCollapsed}
    style={!sidebarCollapsed ? `width:${sidebarWidth}px` : ''}>

    <button class="sb-tog" on:click={() => { sidebarCollapsed = !sidebarCollapsed; }}
        title={sidebarCollapsed ? (isEN ? 'Expand sidebar' : 'Expandir sidebar') : (isEN ? 'Collapse sidebar' : 'Colapsar sidebar')}>
        {sidebarCollapsed ? '›' : '‹'}
        {#if !sidebarCollapsed}<span class="sb-togtxt">{isEN ? 'Collapse' : 'Colapsar'}</span>{/if}
    </button>

    <!-- ── Sistema (collapsible — Sprint 1 UI-2) ── -->
    <div class="sb-lbl sb-accordion-hdr" role="button" tabindex="0"
         on:click={() => toggleSection('sistema')}
         on:keydown={(e) => e.key === 'Enter' && toggleSection('sistema')}>
        {#if !sidebarCollapsed}
            <span>{isEN ? 'System' : 'Sistema'}</span>
            <span class="sb-accordion-arrow" class:open={sistemaOpen}>{sistemaOpen ? '▾' : '▸'}</span>
        {:else}
            <span style="font-size:10px;">≡</span>
        {/if}
    </div>
    {#if sistemaOpen || sidebarCollapsed}
    <div class="sb-accordion-body">
    <!-- v1.7.25 — data-concept attributes wire the sidebar items into
         Lucy's 5-concept color palette (memory cyan, security amber,
         ai teal, infra blue, automation violet). Only the items that
         carry semantic weight in Lucy's mental model are tagged; the
         rest stay default. -->
    <div class="sb-it" class:act={activeView==='dashboard'} data-concept="infra" role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'dashboard' })} on:keydown
         title="Dashboard — métricas del sistema">
        <span class="sb-ico"><LayoutDashboard size={20} /></span><span class="sb-txt">Dashboard</span>
    </div>
    <div class="sb-it" class:act={activeView==='terminal'} data-concept="ai" role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'terminal' })} on:keydown
         title="Terminal IA — chat con Lucy">
        <span class="sb-ico"><Sparkles size={20} /></span><span class="sb-txt">Terminal IA</span>
    </div>
    <div class="sb-it" class:act={activeView==='nexshell'} role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'nexshell' })} on:keydown
         title="NexShell — Hosts remotos e infraestructura">
        <span class="sb-ico"><TerminalSquare size={20} /></span>
        <span class="sb-txt">NexShell</span>
        {#if rshellSessions.length > 0 && !sidebarCollapsed}
            <span class="sb-ns-badge">{rshellSessions.filter(s=>s.connected).length}/{rshellSessions.length}</span>
        {/if}
    </div>
    <div class="sb-it" class:act={activeView==='logviewer'} role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'logviewer' })} on:keydown
         title="Log Viewer — revisar archivos de log">
        <span class="sb-ico"><ScrollText size={20} /></span><span class="sb-txt">Log Viewer</span>
    </div>
    <div class="sb-it" class:act={activeView==='inventory'} role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'inventory' })} on:keydown
         title={isEN ? 'Infrastructure Inventory' : 'Inventario — puertos, servicios, software, certificados'}>
        <span class="sb-ico"><Network size={20} /></span><span class="sb-txt">{isEN ? 'Inventory' : 'Inventario'}</span>
    </div>
    <div class="sb-it" class:act={activeView==='compliance'} data-concept="security" role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'compliance' })} on:keydown
         title={isEN ? 'Compliance — CIS Benchmark audits' : 'Compliance — auditorías CIS Benchmark'}>
        <span class="sb-ico"><ShieldCheck size={20} /></span><span class="sb-txt">Compliance</span>
    </div>
    <div class="sb-it" class:act={activeView==='audittrail'} data-concept="security" role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'audittrail' })} on:keydown
         title={isEN ? 'Audit Trail — command history' : 'Auditoría — historial y seguimiento de comandos'}>
        <span class="sb-ico"><ClipboardList size={20} /></span><span class="sb-txt">{isEN ? 'Audit Trail' : 'Auditoría'}</span>
    </div>
    <div class="sb-it" class:act={activeView==='memory'} data-concept="memory" role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'memory' })} on:keydown
         title={isEN ? 'Memory Browser — memories, crystals, insights, graph' : 'Explorador de Memoria — memorias, cristales, insights, grafo'}>
        <span class="sb-ico"><Brain size={20} /></span><span class="sb-txt">{isEN ? 'Memory' : 'Memoria'}</span>
    </div>
    <div class="sb-it" class:act={activeView==='capacity'} data-concept="infra" role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'capacity' })} on:keydown
         title={isEN ? 'Capacity Planning — historical trends & projections' : 'Capacidad — tendencias históricas y proyecciones'}>
        <span class="sb-ico"><TrendingUp size={20} /></span><span class="sb-txt">{isEN ? 'Capacity' : 'Capacidad'}</span>
    </div>
    <div class="sb-it" class:act={activeView==='diagnostics'} data-concept="infra" role="button" tabindex="0"
         on:click={() => dispatch('setview', { view: 'diagnostics' })} on:keydown
         title={isEN ? 'Self-Diagnostics — unified health checks' : 'Auto-Diagnóstico — chequeos de salud unificados'}>
        <span class="sb-ico"><Stethoscope size={20} /></span><span class="sb-txt">{isEN ? 'Diagnostics' : 'Diagnóstico'}</span>
    </div>

    <!-- v1.7.27 — Recent memory ticker. Renders below the Sistema
         items so the operator gets ambient "Lucy is remembering"
         signal at the bottom of the visible Sistema block. -->
    <MemoryFeed {isEN} {sidebarCollapsed}
        on:open={(e) => dispatch('setview', { view: 'memory' })} />
    </div>
    {/if}
    <div class="sb-div"></div>

    <!-- ── Runbooks (collapsible) ── -->
    <div class="sb-lbl sb-accordion-hdr" style="padding-right:14px;"
         role="button" tabindex="0">
        {#if !sidebarCollapsed}
            <!-- Clicking the label toggles; clicking + button opens the modal -->
            <span style="flex:1;cursor:pointer;"
                  on:click={() => toggleSection('runbooks')}
                  on:keydown={(e) => e.key === 'Enter' && toggleSection('runbooks')}
                  role="button" tabindex="0">RUNBOOKS</span>
            <span class="sb-accordion-arrow" class:open={runbooksOpen}
                  on:click={() => toggleSection('runbooks')}
                  on:keydown
                  role="button" tabindex="0">{runbooksOpen ? '▾' : '▸'}</span>
            <button on:click|stopPropagation={() => dispatch('openmodal', { modal: 'newrunbook' })}
                    style="background:none;border:none;color:var(--acc);cursor:pointer;font-size:15px;font-weight:bold;line-height:1;padding:0 5px;"
                    title={isEN ? 'New runbook' : 'Nuevo runbook'}>+</button>
        {:else}
            <span style="font-size:10px;">≡</span>
        {/if}
    </div>
    {#if runbooksOpen || sidebarCollapsed}
    <div class="sb-accordion-body">
    {#if !$runbooks.length && !sidebarCollapsed}
        <div style="padding:4px 14px 8px;font-size:11px;color:#334155;font-style:italic;">{isEN ? 'No runbooks' : 'Sin runbooks'}</div>
    {/if}
    {#each $runbooks as rb}
    <div class="sb-it sb-action-item" role="button" tabindex="0"
         on:click={() => dispatch('runrunbook', { runbook: rb })} on:keydown
         title="Ejecutar: {rb.name} ({rb.steps.length} pasos)">
        <span class="sb-ico">{rb.icon}</span>
        <span class="sb-txt">{rb.name}</span>
        {#if !sidebarCollapsed}
        <div style="display:flex;align-items:center;gap:4px;margin-left:auto;flex-shrink:0;">
            {#if runbookRunning?.rbId === rb.id && runbookRunning.stepIdx >= 0}
                <span style="font-size:9px;color:var(--amber);">paso {runbookRunning.stepIdx+1}/{rb.steps.length}</span>
            {/if}
            <button class="sb-shell-btn" on:click|stopPropagation={() => dispatch('editrunbook', { runbook: rb })} title="Editar">✏</button>
            <button class="sb-rm-btn" on:click|stopPropagation={() => dispatch('deleterunbook', { id: rb.id })} title="Eliminar">✖</button>
        </div>
        {/if}
    </div>
    {/each}
    </div>
    {/if}

    <div class="sb-div"></div>

    <!-- ── Acciones directas (collapsible) ── -->
    <div class="sb-lbl sb-accordion-hdr" style="padding-right:14px;">
        {#if !sidebarCollapsed}
            <span style="flex:1;cursor:pointer;display:inline-flex;align-items:center;gap:6px;"
                  on:click={() => toggleSection('acciones')}
                  on:keydown={(e) => e.key === 'Enter' && toggleSection('acciones')}
                  role="button" tabindex="0">
                <span>{isEN ? 'Direct actions' : 'Acciones directas'}</span>
                <span class="sb-noai-badge" title={isEN ? 'Execute PowerShell directly, no AI' : 'Ejecutan PowerShell directamente, sin IA'}>{isEN ? 'NO AI' : 'SIN IA'}</span>
            </span>
            <span class="sb-accordion-arrow" class:open={accionesOpen}
                  on:click={() => toggleSection('acciones')}
                  on:keydown role="button" tabindex="0">{accionesOpen ? '▾' : '▸'}</span>
            <button on:click|stopPropagation={() => dispatch('openmodal', { modal: 'newaction' })}
                    style="background:none;border:none;color:var(--acc);cursor:pointer;font-size:16px;font-weight:bold;line-height:1;padding:0 5px;"
                    title={isEN ? 'Add direct action' : 'Añadir acción directa'}>+</button>
        {:else}
            <span style="font-size:10px;">≡</span>
        {/if}
    </div>
    {#if accionesOpen || sidebarCollapsed}
    <div class="sb-accordion-body">

    {#each quickActions as accion, i}
    <div class="sb-it sb-action-item" role="button" tabindex="0"
         on:click={() => dispatch('runaction', { action: accion })} on:keydown
         title="Ejecutar directamente: {accion.nombre}">
        <span class="sb-ico">
            {#if ICON_MAP[accion.icono]}
                <svelte:component this={ICON_MAP[accion.icono]} size={18}/>
            {:else}
                <span style="font-size:13px;">{accion.icono}</span>
            {/if}
        </span>
        <span class="sb-txt">{accion.nombre}</span>
        {#if !sidebarCollapsed}
        <button class="sb-edit" on:click|stopPropagation={() => dispatch('editaction', { index: i })}
                title={isEN ? 'Edit' : 'Editar'}>✎</button>
        <button class="sb-del" on:click|stopPropagation={() => dispatch('deleteaction', { index: i })}
                title={isEN ? 'Delete' : 'Eliminar'}>✖</button>
        {/if}
    </div>
    {/each}
    </div>
    {/if}

    <div class="sb-div" style="margin-top:auto;"></div>

    <!-- ── Registros (accordion) ── -->
    <div class="sb-lbl sb-accordion-hdr" role="button" tabindex="0"
         on:click={() => dispatch('toggleregistros')}
         on:keydown={(e) => e.key === 'Enter' && dispatch('toggleregistros')}>
        {#if !sidebarCollapsed}
            <span>Registros</span>
            <span class="sb-accordion-arrow" class:open={registrosOpen}>{registrosOpen ? '▾' : '▸'}</span>
        {:else}
            <span style="font-size:10px;">≡</span>
        {/if}
    </div>
    {#if registrosOpen || sidebarCollapsed}
    <div class="sb-accordion-body">
        <!-- Activity Feed (24h) — vive aquí porque su naturaleza es
             registro/histórico, no acción. Antes estaba sobre Runbooks y
             desplazaba Memoria/Capacidad/Diagnóstico fuera del viewport. -->
        <ActivityFeedWidget {isEN} {sidebarCollapsed}
            on:navigate={(e) => dispatch('setview', { view: e.detail.view })} />
        <div class="sb-it" role="button" tabindex="0"
             on:click={() => dispatch('memoriaabierta')} on:keydown
             title={isEN ? 'Custom commands learned by Lucy' : 'Comandos aprendidos por Lucy'}>
            <span class="sb-ico"><Brain size={18}/></span><span class="sb-txt">{isEN ? 'Commands' : 'Comandos'}</span>
            {#if customCmdCount > 0}<span class="sb-bdg b">{customCmdCount}</span>{/if}
        </div>
        <div class="sb-it" role="button" tabindex="0"
             on:click={() => dispatch('auditabierto')} on:keydown
             title="Abrir audit log en Notepad">
            <span class="sb-ico"><FileCode size={18}/></span><span class="sb-txt">Audit Log</span>
            {#if auditAlerts > 0}<span class="sb-bdg y">{auditAlerts}</span>{/if}
        </div>
        <div class="sb-it" role="button" tabindex="0"
             on:click={() => dispatch('exportarlog')} on:keydown
             title="Exportar audit log a Descargas">
            <span class="sb-ico"><Download size={18}/></span><span class="sb-txt">Exportar Log</span>
        </div>
    </div>
    {/if}

    <div class="sb-div"></div>

    <!-- ── Utilidades ── -->
    <div class="sb-it" role="button" tabindex="0"
         on:click={() => dispatch('openmodal', { modal: 'tutorial' })} on:keydown
         title={isEN ? 'Interactive guided tour' : 'Tour guiado interactivo de Lucy'}>
        <span class="sb-ico"><GraduationCap size={18}/></span><span class="sb-txt">{isEN ? 'Show Tutorial' : 'Ver Tutorial'}</span>
    </div>
    <div class="sb-it" role="button" tabindex="0"
         on:click={() => dispatch('openmodal', { modal: 'permissions' })} on:keydown
         title={isEN ? 'Manage permission rules' : 'Gestionar reglas de permisos'}>
        <span class="sb-ico"><ShieldCheck size={18}/></span><span class="sb-txt">{isEN ? 'Permissions' : 'Permisos'}</span>
    </div>
    <!-- Skills module disabled — never reached production-ready behaviour and
         duplicates Runbooks functionality. Hidden from the sidebar pending a
         decision: rebuild on top of MCP servers, or remove entirely (Sprint 6+).
         The modal handler stays wired in case we want to re-enable quickly. -->
    <!--
    <div class="sb-it" role="button" tabindex="0"
         on:click={() => dispatch('openmodal', { modal: 'skills' })} on:keydown
         title={isEN ? 'Manage skills' : 'Gestionar skills y runbooks'}>
        <span class="sb-ico"><Zap size={18}/></span><span class="sb-txt">{isEN ? 'Skills' : 'Skills'}</span>
    </div>
    -->
    {#if false}<Zap size={1}/>{/if}
    <div class="sb-it" role="button" tabindex="0"
         on:click={() => dispatch('openmodal', { modal: 'principles' })} on:keydown
         title={isEN ? 'Behavioral principles' : 'Principios — reglas que Lucy sigue'}>
        <span class="sb-ico"><Tag size={18}/></span><span class="sb-txt">{isEN ? 'Principles' : 'Principios'}</span>
    </div>
    <div class="sb-it" role="button" tabindex="0"
         on:click={() => dispatch('openmodal', { modal: 'schedules' })} on:keydown
         title={isEN ? 'Scheduled tasks' : 'Tareas programadas'}>
        <span class="sb-ico"><Bell size={18}/></span><span class="sb-txt">{isEN ? 'Schedules' : 'Programadas'}</span>
    </div>
    <div class="sb-it" role="button" tabindex="0"
         on:click={() => dispatch('toggleforks')} on:keydown
         title={isEN ? 'Sub-Agent Monitor' : 'Monitor de Sub-Agentes'}
         class:sb-it-active={showForksMonitor}>
        <span class="sb-ico"><Brain size={18}/></span><span class="sb-txt">{isEN ? 'Sub-Agents' : 'Sub-Agentes'}</span>
    </div>
    <div class="sb-it" role="button" tabindex="0"
         on:click={() => dispatch('togglepdf')} on:keydown
         title={isEN ? 'PDF Intelligence — Ingest manuals & docs' : 'PDF Intelligence — Ingresa manuales y docs'}
         class:sb-it-active={showPdfPanel}>
        <span class="sb-ico"><FilePdf size={18}/></span><span class="sb-txt">{isEN ? 'PDF Docs' : 'PDF Docs'}</span>
    </div>
    <div class="sb-it" role="button" tabindex="0"
         on:click={() => dispatch('openmodal', { modal: 'settings' })} on:keydown
         title={isEN ? 'Settings & Preferences' : 'Configuración y Preferencias'}>
        <span class="sb-ico"><Settings size={18}/></span><span class="sb-txt">{isEN ? 'Settings' : 'Configuración'}</span>
    </div>
</aside>

{#if !sidebarCollapsed}
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="sb-resize-handle" class:resizing={sidebarResizing}
     on:mousedown|preventDefault={(e) => dispatch('sbresizestart', { event: e })}
     title="Arrastrar para ajustar ancho" on:keydown></div>
{/if}

<style>
    .sidebar{
        display:flex;flex-direction:column;background:#12141e;border-right:1px solid var(--bdr);
        overflow-y:auto;overflow-x:hidden;
        transition:width .32s cubic-bezier(0.16, 1, 0.3, 1);
        flex-shrink:0;padding:8px 0 6px;
        will-change:width;
    }
    .sidebar .sb-it,.sidebar .sb-lbl,.sidebar .sb-div{
        transition: padding .28s cubic-bezier(0.16, 1, 0.3, 1),
                    opacity  .22s ease,
                    transform .28s cubic-bezier(0.16, 1, 0.3, 1);
    }
    .sidebar .sb-txt{transition: opacity .14s ease;}
    .sidebar.closed .sb-txt{opacity:0;}
    /* v1.5.6 — width rules removed from this scoped block. They were
       silently overriding the sidebar.css copy (v1.4.21+ pattern: when
       Svelte adds class-hash to a plain scoped selector, it wins
       cascade over a non-hashed copy in a global module). The inline
       `style="width:${sidebarWidth}px"` on the open state and the
       sidebar.css `.sidebar.closed{width:46px}` rule now drive sizing
       end-to-end. */
    .sb-tog{background:none;border:none;color:var(--txt2);cursor:pointer;font-size:12px;padding:4px 10px;margin-bottom:6px;display:flex;align-items:center;gap:5px;border-radius:4px;transition:.15s;width:100%;}
    .sb-tog:hover{background:rgba(255,255,255,.04);color:var(--txt);}
    .sb-togtxt{font-size:11px;white-space:nowrap;}
    .sb-lbl{font-size:9.5px;color:#64748b;letter-spacing:1.4px;padding:10px 14px 5px;text-transform:uppercase;font-weight:800;white-space:nowrap;display:flex;align-items:center;gap:8px;}
    .sb-lbl::after{content:'';flex:1;height:1px;background:linear-gradient(to right, rgba(100,116,139,.18), transparent);}
    .sidebar.closed .sb-lbl{display:none;}
    .sb-div{height:1px;background:linear-gradient(to right, transparent, var(--bdr) 20%, var(--bdr) 80%, transparent);margin:8px 10px;opacity:.7;}
    .sidebar.closed .sb-div{margin:8px 6px;}
    .sb-it{display:flex;align-items:center;gap:8px;padding:6px 14px;padding-left:16px;font-size:12px;color:var(--txt2);cursor:pointer;transition:background .12s,color .12s;white-space:nowrap;position:relative;}
    .sb-it::before{content:'';position:absolute;left:0;top:18%;bottom:18%;width:2px;border-radius:1px;background:var(--acc);transform:scaleY(0);transform-origin:center;transition:transform .18s cubic-bezier(.4,0,.2,1),opacity .15s;opacity:0;pointer-events:none;}
    .sb-it:hover{background:rgba(16,185,129,.03);color:#94a3b8;}
    .sb-it:hover::before{transform:scaleY(.55);opacity:.38;}
    .sb-it.act{background:rgba(16,185,129,.05);color:var(--acc);}
    .sb-it.act::before{transform:scaleY(1);opacity:1;}
    .sidebar.closed .sb-it{justify-content:center;padding:7px 0;}
    .sidebar.closed .sb-it::before{display:none;}
    .sidebar.closed .sb-it:hover{background:rgba(16,185,129,.05);}
    .sb-it-active{background:rgba(99,102,241,.12)!important;color:#818cf8!important;}
    .sb-it-active::before{background:#818cf8!important;transform:scaleY(1)!important;opacity:1!important;}
    .sb-action-item{position:relative;}
    .sb-del{position:absolute;right:10px;background:transparent;border:none;color:var(--red);opacity:0;cursor:pointer;transition:0.2s;font-size:10px;padding:2px 4px;border-radius:3px;}
    .sb-edit{position:absolute;right:30px;background:transparent;border:none;color:var(--txt2);opacity:0;cursor:pointer;transition:0.2s;font-size:11px;padding:2px 4px;border-radius:3px;}
    .sb-action-item:hover .sb-del,.sb-action-item:hover .sb-edit{opacity:1;}
    .sb-del:hover{background:rgba(239,68,68,.12);}
    .sb-edit:hover{background:rgba(16,185,129,.10);color:var(--acc);}
    .sidebar.closed .sb-del,.sidebar.closed .sb-edit{display:none;}
    .sb-shell-btn{opacity:0;transition:.15s;font-size:11px;color:var(--acc);background:rgba(16,185,129,.06);border:1px solid rgba(16,185,129,.2);border-radius:4px;padding:3px 7px;cursor:pointer;flex-shrink:0;min-width:24px;text-align:center;}
    .sb-shell-btn:hover{background:rgba(16,185,129,.16);}
    .sb-rm-btn{opacity:0;transition:.15s;font-size:11px;color:var(--red);background:rgba(255,68,68,.06);border:1px solid rgba(255,68,68,.2);border-radius:4px;padding:3px 7px;cursor:pointer;flex-shrink:0;min-width:24px;text-align:center;}
    .sb-rm-btn:hover{background:rgba(255,68,68,.16);}
    .sb-action-item:hover .sb-shell-btn,.sb-action-item:hover .sb-rm-btn{opacity:1;}
    .sb-ico{font-size:13px;width:16px;text-align:center;flex-shrink:0;}
    .sb-txt{flex:1;}
    .sidebar.closed .sb-txt{display:none;}
    .sb-noai-badge{display:inline-block;font-size:9px;font-weight:700;letter-spacing:.4px;background:rgba(255,170,0,.1);color:var(--amber);border:1px solid rgba(255,170,0,.2);border-radius:4px;padding:1px 5px;margin-left:6px;vertical-align:middle;text-transform:uppercase;cursor:default;}
    .sb-bdg{font-size:10px;padding:1px 6px;border-radius:10px;flex-shrink:0;}
    .sidebar.closed .sb-bdg{display:none;}
    /* .sb-bdg.g removed — was never applied (no element uses both classes). */
    .sb-bdg.y{background:rgba(255,170,0,.12);color:var(--amber);}
    .sb-bdg.b{background:rgba(59,130,246,.12);color:var(--blue);}
    .sb-ns-badge{font-size:9px;font-weight:700;padding:1px 6px;border-radius:8px;background:rgba(16,185,129,.1);color:var(--acc);flex-shrink:0;}
    .sb-accordion-hdr{cursor:pointer;justify-content:space-between;}
    .sb-accordion-arrow{font-size:10px;margin-left:auto;transition:transform .2s;}
    .sb-accordion-arrow.open{transform:none;}
    :global(.sb-resize-handle){
        width:4px;cursor:col-resize;background:transparent;
        transition:background .15s;flex-shrink:0;
    }
    :global(.sb-resize-handle:hover),:global(.sb-resize-handle.resizing){
        background:rgba(16,185,129,.3);
    }
    :global(:root.light .sb-lbl::after){background:linear-gradient(to right, rgba(15,23,42,.15), transparent);}
    :global(:root.light .sb-lbl){color:#94a3b8;font-weight:800 !important;}
    :global(:root.light .sb-it){color:#cbd5e1;}
    :global(:root.light .sb-it:hover){background:rgba(255,255,255,.06);color:#ffffff;}
    :global(:root.light .sb-it.act){background:color-mix(in srgb, var(--acc) 15%, transparent);color:var(--acc);font-weight:600;}
    :global(:root:not(.light)) .sidebar{
        background:var(--sidebar-overlay, #12141e) !important;
        backdrop-filter:blur(12px) saturate(140%);
        -webkit-backdrop-filter:blur(12px) saturate(140%);
        border-right:1px solid var(--border-glass, var(--bdr)) !important;
        transition:background-color .5s ease;
    }
</style>
