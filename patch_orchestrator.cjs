const fs = require('fs');

let c = fs.readFileSync('src/routes/+page.svelte', 'utf8');

// 1. FILE_TOOL_RE
const fileREOg = "/<TOOL>(readfile|readlines|writefile|listdir|searchfiles|editfile|locate_file|start_indexer|analyze_code):/i";
const fileRERep = "/<TOOL>(readfile|readlines|writefile|listdir|searchfiles|editfile|locate_file|start_indexer|analyze_code|mcp_query|fork_task|wait_task):/i";
if(c.includes(fileREOg)) c = c.replace(fileREOg, fileRERep);

// 2. backgroundTasks
if(!c.includes('let backgroundTasks = {};')) {
    c = c.replace("let pendingLearnSpeak  = false;", "let pendingLearnSpeak  = false;\n    let backgroundTasks = {};");
}

// 3. Tools insertion
const analyzeEndOg = "lucyText = lucyText.replace(/<TOOL>analyze_code:[^<]+<\\/TOOL>/gi, '');\n                        readOnlyTasks.push({ label: `[🌳 AST] ${acAST[1].trim()}`, fn: () => retryWithBackoff(() => invoke('analyze_code', {path:acAST[1].trim()}), 2, true).then(c => `[AST RESULT: ${acAST[1].trim()}]\\n${c}`) });\n                    }";

const insertTools = `
                    for (const mcpQ of [...agentResp.matchAll(/<TOOL>mcp_query:([^|]+)\\|\\|\\|([\\s\\S]*?)<\\/TOOL>/gi)]) {
                        toolUsed = true;
                        lucyText = lucyText.replace(/<TOOL>mcp_query:[\\s\\S]*?<\\/TOOL>/gi, '');
                        readOnlyTasks.push({ label: \`[MCP \${mcpQ[1].trim()}]\`, fn: () => retryWithBackoff(() => invoke('call_mcp_tool', {serverName:mcpQ[1].trim(), query:mcpQ[2].trim()}), 2, true).then(c => \`[MCP \${mcpQ[1].trim()} RESULT]\\n\`+c) });
                    }
                    for (const fork of [...agentResp.matchAll(/<TOOL>fork_task:([^|]+)\\|\\|\\|([\\s\\S]*?)<\\/TOOL>/gi)]) {
                        toolUsed = true;
                        lucyText = lucyText.replace(/<TOOL>fork_task:[\\s\\S]*?<\\/TOOL>/gi, '');
                        const tid = fork[1].trim(); const inst = fork[2].trim();
                        backgroundTasks[tid] = invoke('ask_lucy', {prompt: \`[SUB-AGENT TASK]: \${inst}\`, context: agentCtx.substring(Math.max(0, agentCtx.length - 8000)), userName: lucyConfig.name, runbooksDir: lucyConfig.runbooksDir, model: 'gemini-2.5-flash', lang: userLang, hostsJson: JSON.stringify(hosts), images: null });
                        stepsHtml += \`\\n<span style='font-size:11px;color:#88a;'><br>[🤖 Sub-Agent Forked: \${tid}]</span>\`;
                    }
                    for (const w of [...agentResp.matchAll(/<TOOL>wait_task:([^<]+)<\\/TOOL>/gi)]) {
                        toolUsed = true;
                        lucyText = lucyText.replace(/<TOOL>wait_task:[^<]+<\\/TOOL>/gi, '');
                        const tid=w[1].trim();
                        if(backgroundTasks[tid]){
                            readOnlyTasks.push({ label: \`[⏱️ Wait Task: \${tid}]\`, fn: () => backgroundTasks[tid].then(c => \`[SUB-AGENT \${tid} RESULT]\\n\`+c).catch(e => \`[SUB-AGENT \${tid} ERROR]\\n\`+e) });
                        } else {
                            readOnlyTasks.push({ label: \`[❌ Wait Task: \${tid}]\`, fn: () => Promise.resolve(\`[WAIT_TASK ERROR] El sub-agente '\${tid}' NO EXISTE o nunca fue iniciado.\\nRevisa exacto el formato <TOOL>fork_task... y espera los resultados antes de seguir.\`) });
                        }
                    }`;

const idxAc = c.indexOf("readOnlyTasks.push({ label: `[🌳 AST]");
if(idxAc !== -1 && !c.includes('mcp_query:([^|]+)')) {
   const endBracket = c.indexOf("}", idxAc);
   c = c.substring(0, endBracket + 1) + '\n' + insertTools + c.substring(endBracket + 1);
}

// 4. Fallback parser
const rCat = "const results = await Promise.allSettled(readOnlyTasks.map(t2 => t2.fn()));";
let rCatRep = `if (lucyText.match(/<TOOL>.*?<\\/TOOL>/i)) {
                            toolUsed = true;
                            const unparsed = [...lucyText.matchAll(/<TOOL>([\\s\\S]*?)<\\/TOOL>/gi)];
                            unparsed.forEach(up => {
                                let upN = up[1].trim();
                                readOnlyTasks.push({ label: \`[❌ Error Sintaxis]\`, fn: () => Promise.resolve(\`[SYNTAX ERROR] Se detectó '<TOOL>\${upN}</TOOL>' pero tiene formato inválido o herramienta desconocida. Formatea correctamente.\`) });
                                lucyText = lucyText.replace(up[0], '');
                            });
                        }
                        const results = await Promise.allSettled(readOnlyTasks.map(t2 => t2.fn()));`;
if(c.includes(rCat) && !c.includes('Error Sintaxis')) {
    c = c.replace(rCat, rCatRep);
}

// 5. MAX_LOOPS
if(!c.includes('let identicalErrorCount = 0;')) {
    c = c.replace("const MAX_LOOPS = 15;", "const MAX_LOOPS = 100;\n                let identicalErrorCount = 0;\n                let lastErrorOpsStr = '';\n                let lastThoughtText = '';");
    c = c.replace(/EL Agente Aut.nomo agot. su m.ximo de iteraciones permitidas \(\${MAX_LOOPS}\)/g, "El Agente Autónomo agotó su máximo de iteraciones permitidas (\\${MAX_LOOPS})");
}

// 6. Anti stuck loops
const hitLine = "const hitLimit = (typeof loop_i !== 'undefined') && loop_i >= MAX_LOOPS - 1;";
const hitRep = "const hitLimit = (typeof loop_i !== 'undefined') && (loop_i >= MAX_LOOPS - 1 || identicalErrorCount > 4);";
if(!c.includes('identicalErrorCount > 4')) c = c.replace(hitLine, hitRep);

const errRender = "const errorOps = results.filter(r => r.status === 'rejected' || (typeof r.value === 'string' && r.value.includes('ERROR')));";
const errRep = `const errorOps = results.filter(r => r.status === 'rejected' || (typeof r.value === 'string' && r.value.includes('ERROR')));
                          const errorStr = JSON.stringify(errorOps);
                          if (errorOps.length > 0 && errorStr === lastErrorOpsStr) identicalErrorCount++;
                          else { identicalErrorCount = 0; lastErrorOpsStr = errorStr; }`;
if(!c.includes("identicalErrorCount++")) {
   c = c.replace(errRender, errRep);
}

// 7. Thought handler 
const thOg = "const thM = agentResp.match(/<THOUGHT>([\\s\\S]*?)(?:<\\/THOUGHT>|$)/i);\n                      if (thM) {\n                          thoughtsAccum += thM[1].trim() + '\\n\\n';\n                          lucyText = lucyText.replace(/<THOUGHT>[\\s\\S]*?(?:<\\/THOUGHT>|$)/gi, '');\n                      }";
const thoughtReplace = `const thM = agentResp.match(/<THOUGHT>([\\s\\S]*?)(?:<\\/THOUGHT>|$)/i);
                      if (thM) {
                          const rawT = thM[1].trim();
                          if (rawT === lastThoughtText) {
                              thoughtsAccum += '\\n[o Pensamiento repetido... omitido]\\n\\n';
                          } else {
                              thoughtsAccum += rawT + '\\n\\n';
                              lastThoughtText = rawT;
                          }
                          toolUsed = true; // Svelte implicitly set this originally when thought changed UI
                          lucyText = lucyText.replace(/<THOUGHT>[\\s\\S]*?(?:<\\/THOUGHT>|$)/gi, '');
                      }`;
if(c.includes("thoughtsAccum += thM[1].trim()")) {
    c = c.replace(thOg, thoughtReplace);
}

fs.writeFileSync('src/routes/+page.svelte', c);
console.log('Script v5 executed');
