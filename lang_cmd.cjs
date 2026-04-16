const fs = require('fs');

let s = fs.readFileSync('src/lib/CommandPalette.svelte', 'utf8');

if (!s.includes('export let isEN')) {
    s = s.replace('export let show = false;', "export let show = false;\n    export let isEN = false;");
}

const replacements = [
    ["placeholder=\"Buscar comandos, acciones, hosts...\"", "placeholder={isEN ? 'Search commands, actions, hosts...' : 'Buscar comandos, acciones, hosts...'}"],
    ["Sin resultados para", "{isEN ? 'No results for' : 'Sin resultados para'}"],
    ["<span>↑↓ navegar</span>", "<span>↑↓ {isEN ? 'navigate' : 'navegar'}</span>"],
    ["<span>↵ ejecutar</span>", "<span>↵ {isEN ? 'execute' : 'ejecutar'}</span>"],
    ["<span>Ctrl+P cerrar</span>", "<span>Ctrl+P {isEN ? 'close' : 'cerrar'}</span>"],
];

let updated = s;
for (const [es, eq] of replacements) {
    updated = updated.split(es).join(eq);
}

fs.writeFileSync('src/lib/CommandPalette.svelte', updated, 'utf8');
console.log('CommandPalette translated!');
