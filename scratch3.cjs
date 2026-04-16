const fs = require('fs');

let s = fs.readFileSync('src/routes/+page.svelte', 'utf8');

// Update FILE_TOOL_RE to include graphify and memoria_guardar
s = s.replace(/\|start_indexer\|analyze_code\|mcp_query\):\/i/, '|start_indexer|analyze_code|mcp_query|graphify|memoria_guardar):/i');

// Find the end of the mcp_query loop to append our new tools
const mcpMatch = `readOnlyTasks.push({ label: \`[MCP \${mcpQ[1].trim()}]\`, fn: () => retryWithBackoff(() => invoke('call_mcp_tool', {serverName:mcpQ[1].trim(), query:mcpQ[2].trim()}), 2, true).then(c => \`[MCP \${mcpQ[1].trim()} RESULT]\\n\`+c) });
                        }`;

const extensions = `
                        for (const g of [...agentResp.matchAll(/<TOOL>graphify:([\\s\\S]*?)<\\/TOOL>/gi)]) {
                            toolUsed = true;
                            lucyText = lucyText.replace(/<TOOL>graphify:[\\s\\S]*?<\\/TOOL>/gi, '');
                            const cmd = g[1].trim().replace(/"/g, '\\"');
                            readOnlyTasks.push({ label: '[Graphify]', fn: () => invoke('execute_powershell', {script: \`graphify \${cmd}\`, forceExecute: false}).then(c => '[GRAPHIFY RESULT]\\n'+c).catch(e => '[GRAPHIFY ERROR]\\n'+e) });
                        }

                        for (const m of [...agentResp.matchAll(/<TOOL>memoria_guardar:([\\s\\S]*?)<\\/TOOL>/gi)]) {
                            toolUsed = true;
                            lucyText = lucyText.replace(/<TOOL>memoria_guardar:[\\s\\S]*?<\\/TOOL>/gi, '');
                            const mem = m[1].trim();
                            readOnlyTasks.push({ label: '[Guardando Memoria]', fn: async () => {
                                await invoke('execute_powershell', {script: \`if (!(Test-Path .lucy)) { New-Item -ItemType Directory -Path .lucy | Out-Null }; Add-Content -Path .lucy\\\\workspace_memory.md -Value "\\n--- \\n$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')\\n\${mem.replace(/"/g, "'")}"\`, forceExecute: false});
                                return '[MEMORIA_GUARDADA] Éxito.';
                            }});
                        }
`;

s = s.replace(mcpMatch, mcpMatch + '\n' + extensions);

fs.writeFileSync('src/routes/+page.svelte', s, 'utf8');
console.log('+page.svelte tools successfully extended.');
