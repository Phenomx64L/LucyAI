const fs = require('fs');
let s = fs.readFileSync('src/lib/NexShellView.svelte', 'utf8');

s = s.replace("<option value=\"status\">🟢 {isEN ? 'Status' : '{isEN ? \\'Status\\' : \\'Estado\\'}'}</option>", "<option value=\"status\">🟢 {isEN ? 'Status' : 'Estado'}</option>");
s = s.replace("<option value=\"name\">A-Z {isEN ? 'Name' : '{isEN ? \\'Name\\' : \\'Nombre\\'}'}</option>", "<option value=\"name\">A-Z {isEN ? 'Name' : 'Nombre'}</option>");
s = s.replace("<option value=\"type\">🏷 {isEN ? 'Type' : '{isEN ? \\'Type\\' : \\'Tipo\\'}'}</option>", "<option value=\"type\">🏷 {isEN ? 'Type' : 'Tipo'}</option>");
s = s.replace("<option value=\"activity\">⏱ {isEN ? 'Activity' : '{isEN ? \\'Activity\\' : \\'Actividad\\'}'}</option>", "<option value=\"activity\">⏱ {isEN ? 'Activity' : 'Actividad'}</option>");

fs.writeFileSync('src/lib/NexShellView.svelte', s);
console.log('Fixed NexShellView compilation error');
