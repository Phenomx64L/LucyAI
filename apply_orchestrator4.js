const fs = require('fs');
let lines = fs.readFileSync('src/routes/+page.svelte', 'utf8').split('\n');

for (let i = 0; i < lines.length; i++) {
    // 1. FILE_TOOL_RE
    if (lines[i].includes('const FILE_TOOL_RE = /<TOOL>(readfile|readlines|writefile|listdir|searchfiles|editfile|locate_file|start_indexer|analyze_code):/i;')) {
        lines[i] = lines[i].replace('analyze_code):/i', 'analyze_code|mcp_query|fork_task|wait_task):/i');
    }
    
    // 2. backgroundTasks
    if (lines[i].includes('let pendingLearnSpeak  = false;')) {
        if (!lines[i+1].includes('let backgroundTasks')) {
            lines[i] = lines[i] + '\n    let backgroundTasks = {};';
        }
    }

    // 3. Tools injection
    if (lines[i].includes('// Native read-only tools') && lines[i].includes('concurrentes')) {
        if (!lines[i-1].includes('wait_task')) {
            lines[i] = \
                    for (const mcpQ of [...agentResp.matchAll(/<TOOL>mcp_query:([^|]+)\\\\|\\\\|\\\\|([\\\\s\\\\S]*?)<\\/TOOL>/gi)]) {
                        toolUsed = true;
                        lucyText = lucyText.replace(/<TOOL>mcp_query:[\\\\s\\\\S]*?<\\/TOOL>/gi, '');
                        readOnlyTasks.push({ label: \\\[MCP \]\\\, fn: () => retryWithBackoff(() => invoke('call_mcp_tool', {serverName:mcpQ[1].trim(), query:mcpQ[2].trim()}), 2, true).then(c => \\\[MCP \ RESULT]\\\\n\\\+c) });
                    }
                    for (const fork of [...agentResp.matchAll(/<TOOL>fork_task:([^|]+)\\\\|\\\\|\\\\|([\\\\s\\\\S]*?)<\\/TOOL>/gi)]) {
                        toolUsed = true;
                        lucyText = lucyText.replace(/<TOOL>fork_task:[\\\\s\\\\S]*?<\\/TOOL>/gi, '');
                        const tid = fork[1].trim(); const inst = fork[2].trim();
                        backgroundTasks[tid] = invoke('ask_lucy', {prompt: \\\[SUB-AGENT TASK]: \\\\, context: agentCtx.substring(Math.max(0, agentCtx.length - 8000)), userName: lucyConfig.name, runbooksDir: lucyConfig.runbooksDir, model: 'gemini-2.5-flash', lang: userLang, hostsJson: JSON.stringify(hosts), images: null });
                        stepsHtml += \\\\\\\n<span style='font-size:11px;color:#88a;'><br>[?? Sub-Agent Forked: \]</span>\\\;
                    }
                    for (const w of [...agentResp.matchAll(/<TOOL>wait_task:([^<]+)<\\/TOOL>/gi)]) {
                        toolUsed = true;
                        lucyText = lucyText.replace(/<TOOL>wait_task:[^<]+<\\/TOOL>/gi, '');
                        const tid=w[1].trim();
                        if(backgroundTasks[tid]){
                            readOnlyTasks.push({ label: \\\[?? Wait Task: \]\\\, fn: () => backgroundTasks[tid].then(c => \\\[SUB-AGENT \ RESULT]\\\\n\\\+c).catch(e => \\\[SUB-AGENT \ ERROR]\\\\n\\\+e) });
                        } else {
                            readOnlyTasks.push({ label: \\\[? Wait Task: \]\\\, fn: () => Promise.resolve(\\\[WAIT_TASK ERROR] El sub-agente '\' NO EXISTE o nunca fue iniciado.\\\\nRevisa exacto el formato <TOOL>fork_task... y espera los resultados.\\\) });
                        }
                    }

\ + lines[i];
        }
    }

    // 4. Fallback parser
    if (lines[i].includes('const results = await Promise.allSettled(readOnlyTasks.map(t2 => t2.fn()));')) {
        if (!lines[i-1].includes('Error Sintaxis')) {
            lines[i] = \                        if (lucyText.match(/<TOOL>.*?<\\/TOOL>/i)) {
                            toolUsed = true;
                            const unparsed = [...lucyText.matchAll(/<TOOL>([\\\\s\\\\S]*?)<\\/TOOL>/gi)];
                            unparsed.forEach(up => {
                                let upN = up[1].trim();
                                readOnlyTasks.push({ label: \\\[? Error Sintaxis]\\\, fn: () => Promise.resolve(\\\[SYNTAX ERROR] Se detectó '<TOOL>\</TOOL>' pero tiene formato inválido o herramienta desconocida.\\\) });
                                lucyText = lucyText.replace(up[0], '');
                            });
                        }
\ + lines[i];
        }
    }

    // 5. MAX_LOOPS
    if (lines[i].includes('const MAX_LOOPS = 45;')) {
        lines[i] = lines[i].replace('45', '100');
        if (!lines[i+1].includes('let identicalErrorCount')) {
             lines[i] = lines[i] + "\\n                  let identicalErrorCount = 0;\\n                  let lastErrorOpsStr = '';\\n                  let lastThoughtText = '';";
        }
    }

    // 6. Anti stuck loops
    if (lines[i].includes("const hitLimit = (typeof loop_i !== 'undefined') && loop_i >= MAX_LOOPS - 1;")) {
        lines[i] = lines[i].replace("loop_i >= MAX_LOOPS - 1", "(loop_i >= MAX_LOOPS - 1 || identicalErrorCount > 4)");
    }

    if (lines[i].includes("const errorOps = results.filter(r => r.status === 'rejected' || (typeof r.value === 'string' && r.value.includes('ERROR')));")) {
        if (!lines[i+1].includes('identicalErrorCount++')) {
             lines[i] = lines[i] + "\\n                          const errorStr = JSON.stringify(errorOps);\\n                          if (errorOps.length > 0 && errorStr === lastErrorOpsStr) identicalErrorCount++;\\n                          else { identicalErrorCount = 0; lastErrorOpsStr = errorStr; }";
        }
    }

    if (lines[i].includes("statusLine = \\\n\\n> ?? **Se alcanzó el límite de \ pasos**")) {
        lines[i] = "                            statusLine = identicalErrorCount > 4 ? \\n\\n> ?? **Confinamiento de Error (Anti-Stuck)** — 4 repeticiones idénticas abortadas : \\n\\n> ?? **Se alcanzó el límite de  pasos** — la tarea puede estar incompleta;";
    }

    // 7. Thought handler 
    if (lines[i].includes("const thM = agentResp.match(/<THOUGHT>([\\s\\S]*?)(?:<\\/THOUGHT>|$)/i);")) {
        if (lines[i+1].includes("if (thM) {")) {
             if (lines[i+2].includes("thoughtsAccum += thM[1].trim() + '\\n\\n';")) {
                 lines[i+2] = \                          const rawT = thM[1].trim();
                          if (rawT === lastThoughtText) {
                              thoughtsAccum += '\\n[o Pensamiento repetido... omitido]\\n\\n';
                          } else {
                              thoughtsAccum += rawT + '\\n\\n';
                              lastThoughtText = rawT;
                          }
                          toolUsed = true;\;
             }
        }
    }
}

fs.writeFileSync('src/routes/+page.svelte', lines.join('\\n'));
console.log('Script lines executed');
