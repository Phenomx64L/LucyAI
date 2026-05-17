// ── quick-cmds.ts — static lookup tables extracted from +page.svelte ────────
// Imported by +page.svelte and Sidebar.svelte.

import Activity from '@tabler/icons-svelte/icons/activity';


import Globe from '@tabler/icons-svelte/icons/world';


import Lock from '@tabler/icons-svelte/icons/lock';


import ClipboardList from '@tabler/icons-svelte/icons/clipboard-list';


import Trash2 from '@tabler/icons-svelte/icons/trash';


import Brain from '@tabler/icons-svelte/icons/brain';


import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';


import Zap from '@tabler/icons-svelte/icons/bolt';


import Wrench from '@tabler/icons-svelte/icons/tool';


import Terminal from '@tabler/icons-svelte/icons/terminal';


import Server from '@tabler/icons-svelte/icons/server';


import Download from '@tabler/icons-svelte/icons/download';


import Bug from '@tabler/icons-svelte/icons/bug';


import Monitor from '@tabler/icons-svelte/icons/device-desktop';


import Key from '@tabler/icons-svelte/icons/key';


import FolderOpen from '@tabler/icons-svelte/icons/folder-open';


import Bell from '@tabler/icons-svelte/icons/bell';


import Rocket from '@tabler/icons-svelte/icons/rocket';
export interface IconPaletteEntry {
    key: string;
    icon: any;
    label_es: string;
    label_en: string;
}

export const ICON_PALETTE: IconPaletteEntry[] = [
    { key: 'activity',  icon: Activity,      label_es: 'Salud',       label_en: 'Health' },
    { key: 'globe',     icon: Globe,         label_es: 'Red',         label_en: 'Network' },
    { key: 'lock',      icon: Lock,          label_es: 'Bloqueo',     label_en: 'Lock' },
    { key: 'clipboard', icon: ClipboardList, label_es: 'Lista',       label_en: 'List' },
    { key: 'trash',     icon: Trash2,        label_es: 'Eliminar',    label_en: 'Delete' },
    { key: 'brain',     icon: Brain,         label_es: 'Memoria',     label_en: 'Memory' },
    { key: 'shield',    icon: ShieldCheck,   label_es: 'Seguridad',   label_en: 'Security' },
    { key: 'bolt',      icon: Zap,           label_es: 'Rápida',      label_en: 'Quick' },
    { key: 'wrench',    icon: Wrench,        label_es: 'Herramienta', label_en: 'Tool' },
    { key: 'terminal',  icon: Terminal,      label_es: 'Consola',     label_en: 'Console' },
    { key: 'server',    icon: Server,        label_es: 'Servidor',    label_en: 'Server' },
    { key: 'download',  icon: Download,      label_es: 'Descarga',    label_en: 'Download' },
    { key: 'bug',       icon: Bug,           label_es: 'Debug',       label_en: 'Debug' },
    { key: 'monitor',   icon: Monitor,       label_es: 'Pantalla',    label_en: 'Display' },
    { key: 'key',       icon: Key,           label_es: 'Credencial',  label_en: 'Credential' },
    { key: 'folder',    icon: FolderOpen,    label_es: 'Archivos',    label_en: 'Files' },
    { key: 'bell',      icon: Bell,          label_es: 'Alerta',      label_en: 'Alert' },
    { key: 'rocket',    icon: Rocket,        label_es: 'Lanzar',      label_en: 'Launch' },
];

export const ICON_MAP: Record<string, any> = Object.fromEntries(
    ICON_PALETTE.map(p => [p.key, p.icon])
);

// ── PowerToys helper ─────────────────────────────────────────────────────────
const ptScript = (t: string) =>
    `$exe = Get-ChildItem -Path 'C:\\Program Files\\PowerToys' -Filter '*${t}*.exe' -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1; if ($exe) { Start-Process $exe.FullName } else { throw 'Herramienta no encontrada' }`;

export interface CmdRapido {
    claves: string[];
    script: string;
    respuesta: string;
}

export const cmdRapidos: CmdRapido[] = [
    { claves:["reinicia la aplicacion","borrar mis datos","borra mis datos"], script:"RESET_APP", respuesta:"Reiniciando..." },
    { claves:["salud del sistema","revisa el sistema","estado del sistema"], script:"TOOL_SYSINFO", respuesta:"Revisando..." },
    { claves:["silencia","mute","silenciar"], script:"(new-object -com wscript.shell).SendKeys([char]173)", respuesta:"Audio silenciado." },
    { claves:["baja el volumen","menos volumen","bajale"], script:"$sh = new-object -com wscript.shell; 1..5 | % { $sh.SendKeys([char]174) }", respuesta:"Volumen reducido." },
    { claves:["sube el volumen","mas volumen","subele"], script:"$sh = new-object -com wscript.shell; 1..5 | % { $sh.SendKeys([char]175) }", respuesta:"Volumen subido." },
    { claves:["pausa","play","pausar","reanudar"], script:"(new-object -com wscript.shell).SendKeys([char]179)", respuesta:"Reproducción pausada/reanudada." },
    { claves:["siguiente cancion","next","cambiala"], script:"(new-object -com wscript.shell).SendKeys([char]176)", respuesta:"Siguiente pista." },
    { claves:["anterior cancion","prev"], script:"(new-object -com wscript.shell).SendKeys([char]177)", respuesta:"Pista anterior." },
    { claves:["bloquea el equipo","bloquear pc"], script:"rundll32.exe user32.dll,LockWorkStation", respuesta:"Equipo bloqueado." },
    { claves:["suspende el equipo","suspender pc"], script:"rundll32.exe powrprof.dll,SetSuspendState 0,1,0", respuesta:"Equipo en suspensión." },
    { claves:["vacia la papelera","vaciar papelera"], script:"try{Clear-RecycleBin -Force -ErrorAction Stop;'Papelera vaciada.'}catch{if($_.Exception.Message -match 'encontrar'){Write-Output 'La papelera ya estaba vacía.'}else{throw}}", respuesta:"Papelera vaciada." },
    { claves:["limpia el portapapeles","vaciar portapapeles"], script:"Set-Clipboard -Value $null", respuesta:"Portapapeles limpiado." },
    { claves:["limpia el dns","flush dns"], script:"ipconfig /flushdns", respuesta:"Caché DNS purgada." },
    { claves:["abre descargas","mis descargas"], script:"start shell:Downloads", respuesta:"Abriendo descargas." },
    { claves:["abre documentos","mis documentos"], script:"start shell:Personal", respuesta:"Abriendo documentos." },
    { claves:["abre administrador de tareas","task manager"], script:"start taskmgr", respuesta:"Abriendo Task Manager." },
    { claves:["abre configuracion","settings del sistema"], script:"start ms-settings:", respuesta:"Abriendo Configuración." },
    { claves:["abre panel de control"], script:"control", respuesta:"Abriendo Panel de Control." },
    { claves:["explorador de archivos","abre el explorador"], script:"start explorer", respuesta:"Abriendo Explorador." },
    { claves:["extrae el texto","extractor de texto"], script:ptScript('TextExtractor'), respuesta:"Abriendo Extractor de Texto." },
    { claves:["selector de color","color picker"], script:ptScript('ColorPicker'), respuesta:"Abriendo Selector de Color." },
    { claves:["hosts editor","editar hosts"], script:ptScript('HostsFileEditor'), respuesta:"Abriendo editor de Hosts." },
];

export const mapeoApps: Record<string, string> = {
    "word":"winword","excel":"excel","powerpoint":"powerpnt","calculadora":"calc",
    "paint":"mspaint","bloc de notas":"notepad","recortes":"snippingtool",
    "terminal":"wt","consola":"cmd","powershell":"powershell","chrome":"chrome",
    "edge":"msedge","firefox":"firefox","discord":"discord","spotify":"spotify:",
    "whatsapp":"whatsapp:","youtube":"https://www.youtube.com","github":"https://github.com"
};
