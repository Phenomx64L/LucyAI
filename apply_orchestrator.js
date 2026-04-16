const fs = require('fs');
let c = fs.readFileSync('src/routes/+page.svelte', 'utf8');

// 1. Aumentar MAX_LOOPS
c = c.replace('const MAX_LOOPS = 45;', 'const MAX_LOOPS = 100;');
c = c.replace('Anolisis interrumpido:** El Agente Autnomo agot su moximo de iteraciones permitidas ()', 'Anolisis interrumpido:** El Agente Autnomo agot su moximo de iteraciones permitidas (\\)');

// 2. Insertar variables Anti-Stuck y Thought Sync
if (!c.includes('let identicalErrorCount = 0;')) {
    c = c.replace(/const MAX_LOOPS = 100;/g, "const MAX_LOOPS = 100;\n                let identicalErrorCount = 0;\n                let lastErrorOpsStr = '';\n                let lastThoughtText = '';");
}

// 3. Modificar WAIT_TASK para feedback de error estricto
const waitTaskRE = /if\\(backgroundTasks\\[tid\\]\\)\\{\\s*readOnlyTasks\\.push\\(\\{ label: \\[?? Wait Task: \\$\\{tid\\}\\], fn: \\(\\) => backgroundTasks\\[tid\\].*?\\}\\);\\s*\\}/s;
const waitTaskReplace = if(backgroundTasks[tid]) {
        readOnlyTasks.push({ label: \[?? Wait Task: \]\, fn: () => backgroundTasks[tid].then(c => \[SUB-AGENT \ RESULT]\\n\+c).catch(e => \[SUB-AGENT \ ERROR]\\n\+e) });
    } else {
        readOnlyTasks.push({ label: \[? Wait Task: \]\, fn: () => Promise.resolve(\[WAIT_TASK ERROR] El sub-agente \ NO EXISTE o nunca fue iniciado.\) });
    };

if(c.match(waitTaskRE)) {
    c = c.replace(waitTaskRE, waitTaskReplace);
} else {
    // try literal match
    let wt = 'if(backgroundTasks[tid]){\\s*readOnlyTasks\\.push\\(\\{ label: \\[?? Wait Task: \\$\\{tid\\}\\], fn: \\(\\) => backgroundTasks\\[tid\\].then\\(c => \\[SUB-AGENT \\$\\{tid\\} RESULT\\]\\\\n\\+c\\)\\.catch\\(e => \\[SUB-AGENT \\$\\{tid\\} ERROR\\]\\\\n\\+e\\) \\}\\);\\s*\\}';
    let rx = new RegExp(wt, 's');
    c = c.replace(rx, waitTaskReplace);
}

// 4. Modificar el parsing visual del <THOUGHT>
const thoughtRE = /const thM = agentResp\\.match\\(\\/<THOUGHT>\\(\\[\\\\s\\\\S\\]\\*\\?\\)\\(\\?:<\\\\\\/THOUGHT>\\|\\$\\)\\/i\\);\\s*if \\(thM\\) \\{\\s*const chunk = thM\\[1\\]\\.trim\\(\\) \\+ '\\\\n\\\\n';\\s*thoughtsAccum \\+= chunk;\\s*updateReasoning\\(chunk\\);\\s*lucyText = lucyText\\.replace\\(\\/<THOUGHT>\\[\\\\s\\\\S\\]\\*\\?\\(\\?:<\\\\\\/THOUGHT>\\|\\$\\)\\/gi, ''\\);\\s*\\}/;
const thoughtReplace = const thM = agentResp.match(/<THOUGHT>([\\s\\S]*?)(?:<\\/THOUGHT>|$)/i);
                      if (thM) {
                          const rawThought = thM[1].trim();
                          if (rawThought === lastThoughtText) {
                              const chunk = '[Pensamiento repetido... omitido]\\n\\n';
                              thoughtsAccum += chunk;
                              updateReasoning(chunk);
                          } else {
                              const chunk = rawThought + '\\n\\n';
                              thoughtsAccum += chunk;
                              updateReasoning(chunk);
                              lastThoughtText = rawThought;
                          }
                          lucyText = lucyText.replace(/<THOUGHT>[\\s\\S]*?(?:<\\/THOUGHT>|$)/gi, '');
                      };
c = c.replace(thoughtRE, thoughtReplace);

// 5. Catch-All Syntax Fallback
const resultsCatch = /const results = await Promise\\.allSettled\\(readOnlyTasks\\.map\\(t2 => t2\\.fn\\(\\)\\)\\);/;
const resultsCatchReplace = if (lucyText.match(/<TOOL>.*?<\\/TOOL>/i)) {
                            toolUsed = true;
                            const unparsed = [...lucyText.matchAll(/<TOOL>([\\s\\S]*?)<\\/TOOL>/gi)];
                            unparsed.forEach(up => {
                                readOnlyTasks.push({ label: \[? Error Sintaxis]\, fn: () => Promise.resolve(\[SYNTAX ERROR] Se detectó '<TOOL>\</TOOL>' pero tiene formato inválido. El orquestador ignoró tu petición.\) });
                                lucyText = lucyText.replace(up[0], '');
                            });
                        }
                        const results = await Promise.allSettled(readOnlyTasks.map(t2 => t2.fn()));;
if(c.match(resultsCatch)) {
    c = c.replace(resultsCatch, resultsCatchReplace);
}

// 6. Anti-Stuck Loop Breaker
const renderErrors = /const errorStr = JSON.stringify\\(errorOps\\);\\s*if \\(hitLimit\\)/s;
const renderErrorsReplace = const errorStr = JSON.stringify(errorOps);
                          
                          if (errorOps.length > 0 && errorStr === lastErrorOpsStr) {
                              identicalErrorCount++;
                          } else {
                              identicalErrorCount = 0;
                              lastErrorOpsStr = errorStr;
                          }
                          
                          const hitLimit = (typeof loop_i !== 'undefined') && (loop_i >= MAX_LOOPS - 1 || identicalErrorCount > 4);

                          if (hitLimit);
if(!c.includes('identicalErrorCount > 4')) {
    // Try to find where hitLimit is used
    c = c.replace(/const hitLimit = \([^)]+\) && loop_i >= MAX_LOOPS - 1;/, "const hitLimit = (typeof loop_i !== 'undefined') && (loop_i >= MAX_LOOPS - 1 || identicalErrorCount > 4);");
    c = c.replace(/const errorOps = results.filter\(r => r.status === 'rejected'/g, "const errorOps = results.filter(r => r.status === 'rejected');\n                          const errorStr = JSON.stringify(errorOps);\n                          if (errorOps.length > 0 && errorStr === lastErrorOpsStr) identicalErrorCount++; else { identicalErrorCount = 0; lastErrorOpsStr = errorStr; }\n                          ");
}

fs.writeFileSync('src/routes/+page.svelte', c);
console.log('Orchestrator injected successfully');
