import fs from 'fs';

let c = fs.readFileSync('src/routes/+page.svelte', 'utf8');

// 1. MAX_LOOPS
c = c.replace('const MAX_LOOPS = 45;', 'const MAX_LOOPS = 100;');
c = c.replace('Anǭlisis interrumpido:** El Agente Autnomo agot su mǭximo de iteraciones permitidas (${MAX_LOOPS})', 'Anǭlisis interrumpido:** El Agente Autnomo agot su mǭximo de iteraciones permitidas (\\${MAX_LOOPS})');

// 2. Variables
if (!c.includes('let identicalErrorCount = 0;')) {
    c = c.replace(/const MAX_LOOPS = 100;/g, "const MAX_LOOPS = 100;\n                let identicalErrorCount = 0;\n                let lastErrorOpsStr = '';\n                let lastThoughtText = '';");
}

// 3. Tools injection
const injectionPoint = "lucyText = lucyText.replace(/<TOOL>analyze_code:[^<]+<\\/TOOL>/gi, '');\n                        readOnlyTasks.push({ label: `[🔎💭 AST] ${acAST[1].trim()}`, fn: () => retryWithBackoff(() => invoke('analyze_code', {path:acAST[1].trim()}), 2, true).then(c => `[AST RESULT: ${acAST[1].trim()}]\\n${c}`) });\n                    }";
const mcpQueryCode = `
                    for (const mcpQ of [...agentResp.matchAll(/<TOOL>mcp_query:([^|]+)\\|\\|\\|([\\s\\S]*?)<\\/TOOL>/gi)]) {
                        toolUsed = true;
                        lucyText = lucyText.replace(/<TOOL>mcp_query:[\\s\\S]*?<\\/TOOL>/gi, '');
                        readOnlyTasks.push({ label: \`[MCP \${mcpQ[1].trim()}]\`, fn: () => retryWithBackoff(() => invoke('call_mcp_tool', {serverName:mcpQ[1].trim(), query:mcpQ[2].trim()}), 2, true).then(c => \`[MCP \${mcpQ[1].trim()} RESULT]\\n\`+c) });
                    }
                    for (const fork of [...agentResp.matchAll(/<TOOL>fork_task:([^|]+)\\|\\|\\|([\\s\\S]*?)<\\/TOOL>/gi)]) {
                        toolUsed = true;
                        lucyText = lucyText.replace(/<TOOL>fork_task:[\\s\\S]*?<\\/TOOL>/gi, '');
                        const tid = fork[1].trim(); const inst = fork[2].trim();
                        backgroundTasks[tid] = invoke('ask_lucy', {prompt: \`[SUB-AGENT TASK]: \${inst}\`, context: agentCtx.substring(Math.max(0, agentCtx.length - 8000)), userName: lucyConfig.name, runbooksDir: lucyConfig.runbooksDir, model: 'gemini-2.5-flash', lang: userLang, hostsJson: JSON.stringify(get(hosts)), images: null });
                        stepsHtml += \`\\n<span style='font-size:11px;color:#88a;'><br>[🤖 Sub-Agent Forked: \${tid}]</span>\`;
                    }
                    for (const w of [...agentResp.matchAll(/<TOOL>wait_task:([^<]+)<\\/TOOL>/gi)]) {
                        toolUsed = true;
                        lucyText = lucyText.replace(/<TOOL>wait_task:[^<]+<\\/TOOL>/gi, '');
                        const tid=w[1].trim();
                        if(backgroundTasks[tid]){
                            readOnlyTasks.push({ label: \`[⏱️ Wait Task: \${tid}]\`, fn: () => backgroundTasks[tid].then(c => \`[SUB-AGENT \${tid} RESULT]\\n\`+c).catch(e => \`[SUB-AGENT \${tid} ERROR]\\n\`+e) });
                        } else {
                            readOnlyTasks.push({ label: \`[❌ Wait Task: \${tid}]\`, fn: () => Promise.resolve(\`[WAIT_TASK ERROR] El sub-agente '\${tid}' NO EXISTE o nunca fue iniciado.\\nRevisa el formato exacto de <TOOL>fork_task... Y NO repitas esto a ciegas si algo falló antes.\`) });
                        }
                    }`;

// Safely locate using index
const idx = c.indexOf("readOnlyTasks.push({ label: `[🔎💭 AST] ${acAST[1].trim()}`");
if (idx !== -1 && !c.includes('mcp_query:([^|]+)')) {
    const endBracket = c.indexOf("}", idx);
    const firstPart = c.substring(0, endBracket + 1);
    const secondPart = c.substring(endBracket + 1);
    c = firstPart + '\n' + mcpQueryCode + secondPart;
}

// 4. Thought dedup
const thoughtReplace = `const thM = agentResp.match(/<THOUGHT>([\\s\\S]*?)(?:<\\/THOUGHT>|$)/i);
                      if (thM) {
                          const rawT = thM[1].trim();
                          if (rawT === lastThoughtText) {
                              const chunk = '\\n[oPensamiento repetido... omitido]\\n\\n';
                              thoughtsAccum += chunk;
                              updateReasoning(chunk);
                          } else {
                              const chunk = rawT + '\\n\\n';
                              thoughtsAccum += chunk;
                              updateReasoning(chunk);
                              lastThoughtText = rawT;
                          }
                          lucyText = lucyText.replace(/<THOUGHT>[\\s\\S]*?(?:<\\/THOUGHT>|$)/gi, '');
                      }`;

const ogIdx = c.indexOf("const chunk = thM[1].trim() + '\\n\\n';");
if (ogIdx !== -1) {
    const startIdx = c.lastIndexOf("const thM = agentResp.match(/<THOUGHT>", ogIdx);
    const endIdx = c.indexOf("}", ogIdx) + 1;
    if (startIdx !== -1 && endIdx !== 0) {
        c = c.substring(0, startIdx) + thoughtReplace + c.substring(endIdx);
    }
}

// 5. Syntax Fallback
const rCat = "const results = await Promise.allSettled(readOnlyTasks.map(t2 => t2.fn()));";
let rCatRep = `if (lucyText.match(/<TOOL>.*?<\\/TOOL>/i)) {
                            toolUsed = true;
                            const unparsed = [...lucyText.matchAll(/<TOOL>([\\s\\S]*?)<\\/TOOL>/gi)];
                            unparsed.forEach(up => {
                                let upN = up[1].trim();
                                readOnlyTasks.push({ label: \`[❌ Error Sintaxis]\`, fn: () => Promise.resolve(\`[SYNTAX ERROR] Se detectó '<TOOL>\${upN}</TOOL>' pero tiene formato inválido o no existe la herramienta solicitada. El orquestador ignoró tu petición.\`) });
                                lucyText = lucyText.replace(up[0], '');
                            });
                        }
                        const results = await Promise.allSettled(readOnlyTasks.map(t2 => t2.fn()));`;
if(c.includes(rCat) && !c.includes('unparsed.forEach')) {
    c = c.replace(rCat, rCatRep);
}

// 6. Loop anti stuck
const hitLine = "const hitLimit = (typeof loop_i !== 'undefined') && loop_i >= MAX_LOOPS - 1;";
const hitRep = "const hitLimit = (typeof loop_i !== 'undefined') && (loop_i >= MAX_LOOPS - 1 || identicalErrorCount > 4);";
c = c.replace(hitLine, hitRep);

const errRender = "const errorOps = results.filter(r => r.status === 'rejected' || (typeof r.value === 'string' && r.value.includes('ERROR')));";
const errRep = `const errorOps = results.filter(r => r.status === 'rejected' || (typeof r.value === 'string' && r.value.includes('ERROR')));
                          const errorStr = JSON.stringify(errorOps);
                          if (errorOps.length > 0 && errorStr === lastErrorOpsStr) identicalErrorCount++;
                          else { identicalErrorCount = 0; lastErrorOpsStr = errorStr; }`;
if(!c.includes("identicalErrorCount++")) {
   c = c.replace(errRender, errRep);
}

c = c.replace("statusLine = `\\n\\n> 🟨 **Se alcanzó el límite de ${MAX_LOOPS} pasos** — la tarea puede estar incompleta`;", "statusLine = identicalErrorCount > 4 ? `\\n\\n> 🟥 **Confinamiento de Error (Anti-Stuck)** — 4 repeticiones idénticas abortadas` : `\\n\\n> 🟨 **Límite de \\${MAX_LOOPS} pasos**`;");
c = c.replace("statusLine = `\\n\\n> 🟨 **Se alcanz el lmite de ${MAX_LOOPS} pasos** ?\" la tarea puede estar incompleta`;", "statusLine = identicalErrorCount > 4 ? `\\n\\n> 🟥 **Confinamiento de Error (Anti-Stuck)** — 4 repeticiones idénticas abortadas` : `\\n\\n> 🟨 **Límite de \\${MAX_LOOPS} pasos**`;");
c = c.replace(/statusLine = `\\n\\n> .*?Se alcanz.*? pasos\*\* .*/g, "statusLine = identicalErrorCount > 4 ? `\\n\\n> 🟥 **Confinamiento de Error (Anti-Stuck)** — 4 repeticiones idénticas abortadas` : `\\n\\n> 🟨 **Límite de \\${MAX_LOOPS} pasos**`;");

fs.writeFileSync('src/routes/+page.svelte', c);
console.log('Script done');
