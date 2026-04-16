const fs = require('fs');
let s = fs.readFileSync('src/lib/NexShellView.svelte', 'utf8');

const statusLine = "<option value=\"status\">🟢 {isEN ? 'Status' : '{isEN ? \\'Status\\' : \\'Estado\\'}'}</option>";
const nameLine = "<option value=\"name\">A–Z {isEN ? 'Name' : '{isEN ? \\'Name\\' : \\'Nombre\\'}'}</option>";
const typeLine = "<option value=\"type\">🏷 {isEN ? 'Type' : '{isEN ? \\'Type\\' : \\'Tipo\\'}'}</option>";
const actLine = "<option value=\"activity\">⏱ {isEN ? 'Activity' : '{isEN ? \\'Activity\\' : \\'Actividad\\'}'}</option>";

s = s.replace(statusLine, "<option value=\"status\">🟢 {isEN ? 'Status' : 'Estado'}</option>");
s = s.replace(nameLine, "<option value=\"name\">A–Z {isEN ? 'Name' : 'Nombre'}</option>");
s = s.replace(typeLine, "<option value=\"type\">🏷 {isEN ? 'Type' : 'Tipo'}</option>");
s = s.replace(actLine, "<option value=\"activity\">⏱ {isEN ? 'Activity' : 'Actividad'}</option>");

fs.writeFileSync('src/lib/NexShellView.svelte', s, 'utf8');
console.log('Fixed NexShellView compilation error natively.');
