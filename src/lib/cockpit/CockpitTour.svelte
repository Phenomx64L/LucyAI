<script>
  /* ==========================================================================
     Lucy 2.0 — Cockpit tour (v1.7.233)
     The V2 counterpart of V1's TutorialOverlay: a step-by-step guided tour of
     the cockpit. LIVE — advancing a step switches the actual module behind the
     card (via `onModule`), so the user sees the real screen being described,
     not a mockup. First-run auto-open + re-launchable from the titlebar «?».
     Additive, cockpit-only (mounted by CockpitShell, which is dev-gated).
     ========================================================================== */
  import Sparkles from '@tabler/icons-svelte/icons/sparkles';
  import LayoutDashboard from '@tabler/icons-svelte/icons/layout-dashboard';
  import Terminal2 from '@tabler/icons-svelte/icons/terminal-2';
  import ListDetails from '@tabler/icons-svelte/icons/list-details';
  import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';
  import Server from '@tabler/icons-svelte/icons/server';
  import Brain from '@tabler/icons-svelte/icons/brain';
  import Settings from '@tabler/icons-svelte/icons/settings';
  import X from '@tabler/icons-svelte/icons/x';

  let { open = false, onClose = undefined, onModule = undefined } = $props();

  const steps = [
    {
      icon: Sparkles, module: 'terminal', title: 'Bienvenido al cockpit v2.0',
      tag: 'La nueva cara de Lucy: todo lo que hace el agente, visible en un solo lugar.',
      bullets: [
        ['Rail izquierdo', '8 módulos: Dashboard, Terminal IA, NexShell, Logs, Inventario, Compliance, Memoria y Configuración.'],
        ['Dos carriles', 'en Terminal IA: la conversación al centro y el workspace del agente a la derecha (arrastra su borde para redimensionar, o colápsalo desde el titlebar).'],
        ['Tour en vivo', 'al avanzar de paso, el módulo real aparece detrás de esta tarjeta.'],
        ['Salir', 'el botón «← Salir del cockpit» (abajo a la derecha) te devuelve a la interfaz clásica cuando quieras.'],
      ],
      tip: 'Puedes relanzar este tour con el botón «?» del titlebar.',
    },
    {
      icon: Sparkles, module: 'terminal', title: 'Terminal IA — la conversación',
      tag: 'Chat con el agente real, con todo el contexto a la vista.',
      bullets: [
        ['Pestañas', 'cada pestaña es una conversación/terminal persistida (las de sesiones previas son tu historial). «+» crea, «×» cierra.'],
        ['Modelo por pestaña', 'el selector del encabezado agrupa por proveedor e incluye tus modelos locales de Ollama auto-detectados.'],
        ['Respuestas ricas', 'markdown real (tablas, código, listas), bloques «Pensó durante Xs» expandibles y tarjetas de cada comando/archivo que Lucy toca.'],
        ['Acciones al pasar el cursor', 'copiar y editar en tus mensajes; copiar, regenerar y 👍/👎 en los de Lucy.'],
        ['Detener', 'mientras Lucy trabaja, el botón de enviar se convierte en STOP.'],
      ],
    },
    {
      icon: Terminal2, module: 'terminal', title: 'Terminal IA — el composer',
      tag: 'Escribe órdenes, comandos o adjunta archivos.',
      bullets: [
        ['Paleta «/»', 'teclea / y aparece la paleta de comandos (/model, /clear, /memory, /snapshot, /detective, /privacy…). ↑↓ navega, Tab completa, Enter ejecuta, Esc cierra.'],
        ['Adjuntos 📎', 'imágenes y archivos de texto van directo al modelo (multimodal). Puedes enviar solo adjuntos, sin texto.'],
        ['Prompts sugeridos', 'en una conversación vacía verás 4 chips de arranque: salud del sistema, vulnerabilidades, servicios detenidos y errores recientes.'],
      ],
    },
    {
      icon: ListDetails, module: 'terminal', title: 'El workspace del agente',
      tag: 'Los cuatro paneles de la derecha: qué hace Lucy y cómo lo hace.',
      bullets: [
        ['Plan', 'Lucy siembra sus pasos ANTES de ejecutar (pendiente → en curso → hecho), con tiempo total y errores en rojo.'],
        ['Ejecución', 'cada comando con ✓/✕ y exit code, salida colapsable, copiar comando o salida, y filtro «solo errores».'],
        ['Trace', 'el razonamiento fase a fase (🧠 pensar · ⚡ actuar · 👁 observar) con filtros y timeline.'],
        ['Artefactos', 'archivos editados/escritos con diff (+N/−M), copiar ruta y abrir en el Explorador.'],
        ['Exportar / limpiar', 'los botones 📋 y 🗑 de la barra de pestañas exportan el run como reporte o limpian el workspace.'],
      ],
    },
    {
      icon: ShieldCheck, module: 'terminal', title: 'Seguridad — tú siempre apruebas',
      tag: 'Los guardarraíles de V1 siguen intactos en V2.',
      bullets: [
        ['HITL', 'un comando destructivo o de elevación pausa TODO y muestra el panel ámbar de autorización: ves el comando exacto y decides Permitir o Cancelar.'],
        ['Claves API', 'la configuración muestra solo si están configuradas (nunca el valor); viven en el Credential Manager de Windows.'],
        ['NexShell', 'los comandos peligrosos en shells remotos piden confirmación inline antes de tocar el host.'],
      ],
    },
    {
      icon: LayoutDashboard, module: 'dashboard', title: 'Dashboard',
      tag: 'Salud del sistema en tiempo real — local y hosts remotos.',
      bullets: [
        ['Selector de host', 'local + tus hosts de NexShell; métricas cada 5 s (12 s remoto).'],
        ['Métricas vivas', 'CPU/RAM con sparklines, discos, red, temperatura, núcleos y top procesos (alterna CPU/RAM).'],
        ['Alertas derivadas', 'umbrales de CPU/RAM/disco/temperatura, servicios automáticos detenidos y vulnerabilidades detectadas.'],
      ],
    },
    {
      icon: Terminal2, module: 'nexshell', title: 'NexShell',
      tag: 'Shells reales WinRM/SSH, con lenguaje natural incluido.',
      bullets: [
        ['Comando o prosa', 'escribe el comando directo, o pide en español («libera espacio en /var») y Lucy lo traduce al comando correcto para el SO/distro del host.'],
        ['Streaming interactivo', 'salida en vivo, responde y/n o contraseñas sudo desde el mismo input, y botón Detener para procesos colgados.'],
        ['Historial ↑/↓', 'recupera comandos anteriores; las sesiones sobreviven al cambiar de módulo.'],
        ['Gestión de hosts', 'añadir/editar/eliminar hosts desde aquí (credenciales cifradas en el llavero del SO).'],
      ],
    },
    {
      icon: Server, module: 'inventory', title: 'Logs · Inventario · Compliance',
      tag: 'Los tres módulos de auditoría continua.',
      bullets: [
        ['Logs', 'audit trail estructurado con niveles, búsqueda, pausa del refresco y copiar (todo o por línea).'],
        ['Inventario', 'scan local de puertos/servicios/software/certificados + escáner de vulnerabilidades con base CVE offline: severidad, CVSS, versión corregida y comando de parche listo para copiar. Re-scan cada 30 min y aviso del sistema cada 6 h aunque el cockpit esté cerrado.'],
        ['Compliance', 'checks CIS de Windows con score, drill-down por control (comando, salida observada, remediación) y export del informe.'],
      ],
    },
    {
      icon: Brain, module: 'memory', title: 'Memoria y documentos',
      tag: 'Lo que Lucy recuerda — y los manuales que consulta.',
      bullets: [
        ['Stats reales', 'totales del corpus completo, no de la página visible.'],
        ['Memorias', 'busca, revisa importancia (alta/media/baja) y borra al pasar el cursor.'],
        ['Documentos 📄', 'ingiere PDFs (manuales, guías) con progreso en vivo; el chip «N/M embebidos» confirma que la búsqueda semántica está al 100%. Re-ingestar el mismo PDF no duplica nada.'],
        ['Cómo lo usa Lucy', 'los documentos NO inundan la memoria: Lucy los consulta con pdf_search cuando el tema lo pide.'],
      ],
    },
    {
      icon: Settings, module: 'config', title: 'Configuración',
      tag: 'Ajustes del cockpit y del agente.',
      bullets: [
        ['Color de acento', 'seis acentos para el cockpit (independiente del tema clásico).'],
        ['Claves API', 'estado por proveedor + botón Configurar (el flujo seguro clásico).'],
        ['Límite de gasto', 'tope en USD por sesión que el agente respeta.'],
        ['Personalidad', 'conciso / equilibrado / detallado.'],
        ['Modelos locales', 'estado de Ollama y redetección de modelos.'],
        ['Configuración completa →', 'abre el modal clásico con TODO (MCP, temas, iteraciones, idioma…).'],
      ],
      tip: 'Fin del tour — relánzalo cuando quieras con el «?» del titlebar.',
    },
  ];

  let idx = $state(0);
  const step = $derived(steps[idx]);
  const isLast = $derived(idx === steps.length - 1);

  function goto(i) {
    idx = Math.max(0, Math.min(steps.length - 1, i));
    onModule?.(steps[idx].module);
  }
  function next() { if (isLast) onClose?.(); else goto(idx + 1); }
  function prev() { goto(idx - 1); }
  function onKey(e) {
    if (!open) return;
    if (e.key === 'ArrowRight') { e.preventDefault(); next(); }
    else if (e.key === 'ArrowLeft') { e.preventDefault(); prev(); }
    else if (e.key === 'Escape') { e.preventDefault(); onClose?.(); }
  }
  // Reset to step 1 (and sync the module) each time the tour opens.
  $effect(() => { if (open) { idx = 0; onModule?.(steps[0].module); } });
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  {@const Icon = step.icon}
  <div class="tour-scrim" role="dialog" aria-modal="true" aria-label="Tutorial del cockpit">
    <div class="tour-card">
      <button class="tour-x" aria-label="Cerrar tour" onclick={() => onClose?.()}><X size={15} stroke={1.75} /></button>
      <div class="tour-head">
        <span class="tour-icon"><Icon size={20} stroke={1.6} /></span>
        <div class="tour-titles">
          <div class="tour-title">{step.title}</div>
          <div class="tour-tag">{step.tag}</div>
        </div>
      </div>
      <ul class="tour-list">
        {#each step.bullets as [b, t]}
          <li><b>{b}</b> — {t}</li>
        {/each}
      </ul>
      {#if step.tip}<div class="tour-tip">💡 {step.tip}</div>{/if}
      <div class="tour-foot">
        <button class="tour-skip" onclick={() => onClose?.()}>Saltar tour</button>
        <div class="tour-dots" role="tablist" aria-label="Pasos del tour">
          {#each steps as _, i}
            <button class="dot" class:on={i === idx} aria-label="Paso {i + 1}" onclick={() => goto(i)}></button>
          {/each}
        </div>
        <div class="tour-nav">
          {#if idx > 0}<button class="tour-btn" onclick={prev}>← Anterior</button>{/if}
          <button class="tour-btn primary" onclick={next}>{isLast ? 'Terminar ✓' : 'Siguiente →'}</button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Light scrim on purpose: the LIVE module behind must stay readable —
     the tour describes what the user is actually seeing. */
  /* z-55: above every module, BELOW the HITL authorization scrim (z-60) —
     a pending destructive-command approval always outranks the tour. */
  .tour-scrim {
    position: fixed; inset: 0; z-index: 55;
    background: rgba(2, 6, 12, 0.45);
    display: flex; align-items: flex-end; justify-content: center;
    padding: 0 24px 34px;
    animation: tour-in var(--dur-slow) var(--ease-out);
  }
  @keyframes tour-in { from { opacity: 0; } to { opacity: 1; } }
  .tour-card {
    position: relative; width: 640px; max-width: 100%;
    max-height: min(62vh, 520px); overflow-y: auto;
    background: var(--surface-1); border: 1px solid var(--border-strong);
    border-radius: var(--r-lg); padding: 20px 22px 16px;
    box-shadow: 0 18px 50px -12px rgba(0, 0, 0, 0.65);
    animation: tour-card-in var(--dur-slow) var(--ease-out);
  }
  @keyframes tour-card-in { from { opacity: 0; transform: translateY(14px); } to { opacity: 1; transform: none; } }
  .tour-x {
    position: absolute; top: 10px; right: 10px;
    width: 26px; height: 26px; display: flex; align-items: center; justify-content: center;
    border: 0; border-radius: var(--r-sm); background: transparent; color: var(--text-faint); cursor: pointer;
    transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out);
  }
  .tour-x:hover { color: var(--text-primary); background: var(--surface-3); }
  .tour-head { display: flex; gap: 13px; align-items: flex-start; margin-bottom: 12px; }
  .tour-icon {
    flex-shrink: 0; width: 40px; height: 40px; border-radius: var(--r-md);
    display: flex; align-items: center; justify-content: center;
    background: var(--accent-bg); color: var(--accent); border: 1px solid var(--accent-line);
  }
  .tour-title { font-size: var(--fs-title); font-weight: var(--fw-medium); color: var(--text-primary); }
  .tour-tag { font-size: var(--fs-footnote); color: var(--text-muted); margin-top: 3px; line-height: var(--lh-body); }
  .tour-list { margin: 0; padding: 0 0 0 4px; list-style: none; display: flex; flex-direction: column; gap: 8px; }
  .tour-list li { font-size: var(--fs-footnote); line-height: var(--lh-body); color: var(--text-secondary); padding-left: 14px; position: relative; }
  .tour-list li::before { content: '·'; position: absolute; left: 0; color: var(--accent); font-weight: 700; }
  .tour-list b { color: var(--text-primary); font-weight: var(--fw-medium); }
  .tour-tip {
    margin-top: 12px; font-size: var(--fs-caption); color: var(--text-muted);
    background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-md); padding: 7px 11px;
  }
  .tour-foot { display: flex; align-items: center; gap: 12px; margin-top: 16px; }
  .tour-skip {
    border: 0; background: transparent; color: var(--text-faint); font-size: var(--fs-caption);
    cursor: pointer; padding: 4px 2px; font-family: var(--font-sans);
  }
  .tour-skip:hover { color: var(--text-secondary); }
  .tour-dots { flex: 1; display: flex; align-items: center; justify-content: center; gap: 6px; }
  .dot {
    width: 7px; height: 7px; border-radius: var(--r-pill); border: 0; padding: 0; cursor: pointer;
    background: var(--surface-3); transition: background var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-out);
  }
  .dot.on { background: var(--accent); transform: scale(1.25); }
  .tour-nav { display: flex; gap: 8px; }
  .tour-btn {
    font-size: var(--fs-footnote); color: var(--text-secondary); cursor: pointer; font-family: var(--font-sans);
    background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-md); padding: 6px 13px;
    transition: color var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out);
  }
  .tour-btn:hover { color: var(--text-primary); border-color: var(--border-accent); }
  .tour-btn.primary { color: var(--accent-ink); background: var(--accent); border-color: var(--accent); }
  .tour-btn.primary:hover { background: var(--accent-hover); color: var(--accent-ink); }

  @media (prefers-reduced-motion: reduce) {
    .tour-scrim, .tour-card { animation: none; }
  }
</style>
