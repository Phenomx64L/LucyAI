const fs = require('fs');

let s = fs.readFileSync('src/lib/HostModal.svelte', 'utf8');

// 1. Add export let isEN
if (!s.includes('export let isEN')) {
    s = s.replace('export let editingHost = null;', "export let editingHost = null;\n    export let isEN = false;");
}

const replacements = [
    // Modal Titles
    ["{editingHost ? 'Editar Host' : 'Nuevo Host Remoto'}", "{editingHost ? (isEN ? 'Edit Host' : 'Editar Host') : (isEN ? 'New Remote Host' : 'Nuevo Host Remoto')}"],
    
    // Labels
    ["Nombre *", "{isEN ? 'Name *' : 'Nombre *'}"],
    ["Protocolo *", "{isEN ? 'Protocol *' : 'Protocolo *'}"],
    ["Categoría", "{isEN ? 'Category' : 'Categoría'}"],
    ["Motor de base de datos", "{isEN ? 'Database Engine' : 'Motor de base de datos'}"],
    ["Usuario {hostForm.protocol", "{isEN ? 'User ' : 'Usuario '}{hostForm.protocol"],
    ["Contraseña {editingHost ? '(dejar vacío = no cambiar)' : '*'}", "{isEN ? 'Password ' : 'Contraseña '} {editingHost ? (isEN ? '(leave empty = no change)' : '(dejar vacío = no cambiar)') : '*'}"],
    ["separados por coma — ej: prod, web, db", "{isEN ? 'comma separated — e.g. prod, web, db' : 'separados por coma — ej: prod, web, db'}"],
    ["Color del host", "{isEN ? 'Host Color' : 'Color del host'}"],
    ["Ruta de clave SSH privada", "{isEN ? 'Private SSH key path' : 'Ruta de clave SSH privada'}"],
    ["opcional — deja vacío para usar contraseña", "{isEN ? 'optional — leave empty to use password' : 'opcional — deja vacío para usar contraseña'}"],
    ["Si se especifica, se usa <code>ssh -i &lt;ruta&gt;</code> en lugar de contraseña.", "{isEN ? 'If specified, <code>ssh -i &lt;path&gt;</code> will be used instead of a password.' : 'Si se especifica, se usa <code>ssh -i &lt;ruta&gt;</code> en lugar de contraseña.'}"],
    ["Seleccionar archivo de clave", "{isEN ? 'Select key file' : 'Seleccionar archivo de clave'}"],

    // Select Placeholder Options
    ["Servidor / Shell", "{isEN ? 'Server / Shell' : 'Servidor / Shell'}"],
    ["Base de datos", "{isEN ? 'Database' : 'Base de datos'}"],
    ["Contenedor (Docker)", "{isEN ? 'Container (Docker)' : 'Contenedor (Docker)'}"],
    ["Dispositivo de red", "{isEN ? 'Network Device' : 'Dispositivo de red'}"],
    ["Bases de datos", "{isEN ? 'Databases' : 'Bases de datos'}"],
    ["Infraestructura", "{isEN ? 'Infrastructure' : 'Infraestructura'}"],
    ["Acceso remoto", "{isEN ? 'Remote access' : 'Acceso remoto'}"],

    // Placeholders
    ["placeholder=\"Ej. Prod-Web-01\"", "placeholder={isEN ? 'E.g. Prod-Web-01' : 'Ej. Prod-Web-01'}"],
    ["placeholder=\"192.168.1.10 ó servidor.empresa.com\"", "placeholder={isEN ? '192.168.1.10 or server.company.com' : '192.168.1.10 ó servidor.empresa.com'}"],

    // Confirmation dialogues
    ["¿Estás seguro de que deseas eliminar este host? Esta acción no se puede deshacer.", "${isEN ? 'Are you sure you want to delete this host? This action cannot be undone.' : '¿Estás seguro de que deseas eliminar este host? Esta acción no se puede deshacer.'}"],

    // Buttons
    ["🗑️ Eliminar", "🗑️ {isEN ? 'Delete' : 'Eliminar'}"],
    ["Cancelar", "{isEN ? 'Cancel' : 'Cancelar'}"],
    ["hostSaving ? '⏳ Guardando...' : editingHost ? 'Actualizar Host' : 'Guardar Host'", "hostSaving ? (isEN ? '⏳ Saving...' : '⏳ Guardando...') : editingHost ? (isEN ? 'Update Host' : 'Actualizar Host') : (isEN ? 'Save Host' : 'Guardar Host')"],

    // Info Blocks
    ["Requisito SSH:", "{isEN ? 'SSH Requirement:' : 'Requisito SSH:'}"],
    ["El equipo local debe tener OpenSSH\n      instalado (incluido en Windows 10/11) y el host remoto debe permitir autenticación por\n      contraseña o clave SSH.", "{isEN ? 'The local machine must have OpenSSH installed (included in Windows 10/11) and the remote host must allow authentication via password or SSH key.' : 'El equipo local debe tener OpenSSH instalado (incluido en Windows 10/11) y el host remoto debe permitir autenticación por contraseña o clave SSH.'}"],
    ["Requisito WinRM:", "{isEN ? 'WinRM Requirement:' : 'Requisito WinRM:'}"],
    ["El servidor remoto debe tener WinRM\n      habilitado. Ejecuta en el servidor:", "{isEN ? 'The remote server must have WinRM enabled. Run on the server:' : 'El servidor remoto debe tener WinRM habilitado. Ejecuta en el servidor:'}"],
    ["Lucy lanzará una sesión de Escritorio Remoto", "{isEN ? 'Lucy will launch a Remote Desktop session' : 'Lucy lanzará una sesión de Escritorio Remoto'}"],
    ["al conectar.\n      Asegúrate de que el puerto 3389 esté accesible y el acceso remoto habilitado en el servidor.", "{isEN ? 'upon connection. Ensure port 3389 is accessible and remote access is enabled on the server.' : 'al conectar. Asegúrate de que el puerto 3389 esté accesible y el acceso remoto habilitado en el servidor.'}"],
    ["Requiere que el daemon de Docker exponga\n      el API TCP. Configura en", "{isEN ? 'Requires the Docker daemon to expose the TCP API. Configure in' : 'Requiere que el daemon de Docker exponga el API TCP. Configura en'}"],
    ["Usa TLS (2376) en producción.", "{isEN ? 'Use TLS (2376) in production.' : 'Usa TLS (2376) en producción.'}"],
    ["Lucy ejecutará comandos", "{isEN ? 'Lucy will execute' : 'Lucy ejecutará comandos'}"],
    ["contra el API server.\n      Asegúrate de tener un", "{isEN ? 'commands against the API server. Ensure you have a valid' : 'contra el API server. Asegúrate de tener un'}"],
    ["válido\n      o un token de servicio.", "{isEN ? 'valid <code>kubeconfig</code> or a service token.' : 'válido o un token de servicio.'}"],
    ["Lucy realizará consultas SNMP (GET/WALK) al\n      dispositivo. El campo \"Usuario\" se usa como", "{isEN ? 'Lucy will perform SNMP queries (GET/WALK) to the device. The User field is used as the' : 'Lucy realizará consultas SNMP (GET/WALK) al dispositivo. El campo \"Usuario\" se usa como'}"],
    ["o usuario SNMPv3.\n      Puerto estándar: 161.", "{isEN ? 'or SNMPv3 user. Standard port: 161.' : 'o usuario SNMPv3. Puerto estándar: 161.'}"],
    ["Lucy se conectará al motor", "{isEN ? 'Lucy will connect to the' : 'Lucy se conectará al motor'}"],
    ["en el puerto {defaultPort(hostForm.protocol)}. Asegúrate de que el servidor acepte conexiones remotas\n      y que el usuario tenga los permisos necesarios.", "{isEN ? `engine on port ${defaultPort(hostForm.protocol)}. Ensure the server accepts remote connections and the user has required permissions.` : `en el puerto ${defaultPort(hostForm.protocol)}. Asegúrate de que el servidor acepte conexiones remotas y que el usuario tenga los permisos necesarios.`}"],
];

let updated = s;
for (const [es, eq] of replacements) {
    updated = updated.split(es).join(eq);
}

fs.writeFileSync('src/lib/HostModal.svelte', updated, 'utf8');
console.log('HostModal translated!');
