const fs = require('fs');
let s = fs.readFileSync('src/lib/NexShellView.svelte', 'utf8');

const reps = [
    // Modal Toolbars & Inputs
    ["<th>Atajo</th>", "<th>{isEN ? 'Shortcut' : 'Atajo'}</th>"],
    ["<th>Expansión</th>", "<th>{isEN ? 'Expansion' : 'Expansión'}</th>"],
    ["placeholder=\"Nombre del host (ej. Servidor Web 1)\"", "placeholder={isEN ? 'Host name (e.g. Web Server 1)' : 'Nombre del host (ej. Servidor Web 1)'}"],
    ["placeholder=\"Tags, IP, o nombre...\"", "placeholder={isEN ? 'Tags, IP, or name...' : 'Tags, IP, o nombre...'}"],
    ["Configuración de Host", "{isEN ? 'Host Configuration' : 'Configuración de Host'}"],

    // Placeholders
    ["placeholder=\"¿Qué deseas verificar o configurar en este host?\"", "placeholder={isEN ? 'What do you want to verify or configure on this host?' : '¿Qué deseas verificar o configurar en este host?'}"],
    ["placeholder=\"Ej. revisar estado del servicio nginx, o sudo apt update\"", "placeholder={isEN ? 'E.g. check nginx service status, or sudo apt update' : 'Ej. revisar estado del servicio nginx, o sudo apt update'}"],
    ["Enviar instrucción o comando...", "{isEN ? 'Send instruction or command...' : 'Enviar instrucción o comando...'}"],
    ["Limitar a: base de datos, contenedor...", "{isEN ? 'Filter by: database, container...' : 'Limitar a: base de datos, contenedor...'}"],
    
    // Status
    ["Conectado a", "{isEN ? 'Connected to' : 'Conectado a'}"],
    ["Desconectado", "{isEN ? 'Disconnected' : 'Desconectado'}"],
    ["Sesión Activa", "{isEN ? 'Active Session' : 'Sesión Activa'}"],

    // Buttons
    ["Ejecutar script", "{isEN ? 'Run script' : 'Ejecutar script'}"],
    ["Cancelar ejecución", "{isEN ? 'Cancel execution' : 'Cancelar ejecución'}"],
    ["Cerrar sesión", "{isEN ? 'Close session' : 'Cerrar sesión'}"],
    ["Reconectar", "{isEN ? 'Reconnect' : 'Reconectar'}"],
    ["Nueva Conexión", "{isEN ? 'New Connection' : 'Nueva Conexión'}"],

    // Errors
    ["Error al conectar con", "{isEN ? 'Error connecting to' : 'Error al conectar con'}"],
    ["No se pudo resolver", "{isEN ? 'Could not resolve' : 'No se pudo resolver'}"],

    // Tooltips & Categories
    ["Todos los hosts", "{isEN ? 'All hosts' : 'Todos los hosts'}"],
    ["Bases de datos", "{isEN ? 'Databases' : 'Bases de datos'}"],
    ["Servidores Linux", "{isEN ? 'Linux Servers' : 'Servidores Linux'}"],
    ["Servidores Windows", "{isEN ? 'Windows Servers' : 'Servidores Windows'}"],

    // Filters
    ["Filtrar por nombre...", "{isEN ? 'Filter by name...' : 'Filtrar por nombre...'}"],
    ["Ordenar por", "{isEN ? 'Sort by' : 'Ordenar por'}"],
    ["Estado", "{isEN ? 'Status' : 'Estado'}"],
    ["Nombre", "{isEN ? 'Name' : 'Nombre'}"],
    ["Tipo", "{isEN ? 'Type' : 'Tipo'}"],
    ["Actividad", "{isEN ? 'Activity' : 'Actividad'}"],
    
    // Playbook
    ["Guarda la instrucción", "{isEN ? 'Save the instruction' : 'Guarda la instrucción'}"],
    ["Nuevo Playbook", "{isEN ? 'New Playbook' : 'Nuevo Playbook'}"],
    ["Guardar Playbook", "{isEN ? 'Save Playbook' : 'Guardar Playbook'}"],
    ["Nombre para guardar el playbook", "{isEN ? 'Name to save the playbook' : 'Nombre para guardar el playbook'}"],

    // Transfer Modal
    ["Transferencia completada", "{isEN ? 'Transfer complete' : 'Transferencia completada'}"],
    ["Subir archivo al Host", "{isEN ? 'Upload file to Host' : 'Subir archivo al Host'}"],
    ["Descargar desde Host", "{isEN ? 'Download from Host' : 'Descargar desde Host'}"],
    ["Ruta del archivo local", "{isEN ? 'Local file path' : 'Ruta del archivo local'}"],
    ["Ruta de destino remota", "{isEN ? 'Remote destination path' : 'Ruta de destino remota'}"],
    ["Seleccionar", "{isEN ? 'Select' : 'Seleccionar'}"],
    ["Ejecutar transferencia", "{isEN ? 'Execute transfer' : 'Ejecutar transferencia'}"],
];

let updated = s;
for (const [es, eq] of reps) {
    updated = updated.split(es).join(eq);
}

fs.writeFileSync('src/lib/NexShellView.svelte', updated, 'utf8');
console.log('NexShellView headers translated!');
