const fs = require('fs');
let c = fs.readFileSync('src/routes/+page.svelte', 'utf8');

c = c.replace('const MAX_LOOPS = 45;', 'const MAX_LOOPS = 100;');
c = c.replace('El Agente Aut\\u00f3nomo agot\\u00f3 su m\\u00e1ximo de iteraciones permitidas ()', 'El Agente Autónomo agotó su máximo de iteraciones permitidas (\\)');

if(!c.includes('identicalErrorCount = 0')) {
    c = c.replace('let agentResp = resp;', "let agentResp = resp;\n                let identicalErrorCount = 0;\n                let lastErrorOpsStr = '';\n                let lastThoughtText = '';");
}

let waitTaskOriginal = "if(backgroundTasks[tid]){\\s*readOnlyTasks\\.push\\(\\{ label: \\[?? Wait Task: \\$\\{tid\\}\\], fn: \\(\\) => backgroundTasks\\[tid\\].then\\(c => \\[SUB-AGENT \\$\\{tid\\} RESULT\\]\\\\n\\+c\\)\\.catch\\(e => \\[SUB-AGENT \\$\\{tid\\} ERROR\\]\\\\n\\+e\\) \\}\\);\\s*\\}";
let rx = new RegExp(waitTaskOriginal, 's');
let waitTaskReplace = if(backgroundTasks[tid]){
        readOnlyTasks.push({ label: \\\[?? Wait Task: \]\\\, fn: () => backgroundTasks[tid].then(c => \\\[SUB-AGENT \ RESULT]\\\\n\\\+c).catch(e => \\\[SUB-AGENT \ ERROR]\\\\n\\\+e) });
    } else {
        readOnlyTasks.push({ label: \\\[? Wait Task: \]\\\, fn: () => Promise.resolve(\\\[WAIT_TASK ERROR] El sub-agente '\' NO EXISTE o nunca fue iniciado.\\\) });
    };
if(c.match(rx)) c = c.replace(rx, waitTaskReplace);

let ht1 = "const thM = agentResp.match(/<THOUGHT>([\\\\s\\\\S]*?)(?:<\\\\/THOUGHT>|$)/i);";
let ht2 = "if (thM) {";
let ht3 = "const chunk = thM[1].trim() + '\\\\n\\\\n';";
let ht4 = "thoughtsAccum += chunk;";
let ht5 = "updateReasoning(chunk);";
let ht6 = "lucyText = lucyText.replace(/<THOUGHT>[\\\\s\\\\S]*?(?:<\\\\/THOUGHT>|$)/gi, '');";
let rxT = new RegExp(ht1.replace(/[-[\\]{}()*+?.,\\\\^$|#\\s]/g, '\\\\$&') + "\\\\s*" + ht2.replace(/[-[\\]{}()*+?.,\\\\^$|#\\s]/g, '\\\\$&') + "\\\\s*" + ht3.replace(/[-[\\]{}()*+?.,\\\\^$|#\\s]/g, '\\\\$&') + "\\\\s*" + ht4.replace(/[-[\\]{}()*+?.,\\\\^$|#\\s]/g, '\\\\$&') + "\\\\s*" + ht5.replace(/[-[\\]{}()*+?.,\\\\^$|#\\s]/g, '\\\\$&') + "\\\\s*" + ht6.replace(/[-[\\]{}()*+?.,\\\\^$|#\\s]/g, '\\\\$&'));

let authTReplace = const thM = agentResp.match(/<THOUGHT>([\\s\\S]*?)(?:<\\/THOUGHT>|$)/i);
                      if (thM) {
                          const rawThought = thM[1].trim();
                          if (rawThought === lastThoughtText) {
                              const chunk = '\\n[o Pensamiento repetido... omitido]\\n\\n';
                              thoughtsAccum += chunk;
                              updateReasoning(chunk);
                          } else {
                              const chunk = rawThought + '\\n\\n';
                              thoughtsAccum += chunk;
                              updateReasoning(chunk);
                              lastThoughtText = rawThought;
                          }
                          lucyText = lucyText.replace(/<THOUGHT>[\\s\\S]*?(?:<\\/THOUGHT>|$)/gi, '');;

if(c.match(rxT)) c = c.replace(rxT, authTReplace);

let resultsStr = "const results = await Promise.allSettled(readOnlyTasks.map(t2 => t2.fn()));";
let resultsStrRep = if (lucyText.match(/<TOOL>.*?<\\/TOOL>/i)) {
                            toolUsed = true;
                            const unparsed = [...lucyText.matchAll(/<TOOL>([\\s\\S]*?)<\\/TOOL>/gi)];
                            unparsed.forEach(up => {
                                readOnlyTasks.push({ label: \\\[? Error Sintaxis]\\\, fn: () => Promise.resolve(\\\[SYNTAX ERROR] Se detectó '<TOOL>\</TOOL>' pero tiene formato inválido. El orquestador ignoró tu petición.\\\) });
                                lucyText = lucyText.replace(up[0], '');
                            });
                        }
                        const results = await Promise.allSettled(readOnlyTasks.map(t2 => t2.fn()));;
c = c.replace(resultsStr, resultsStrRep);

let renderErrors = "const errorOps = results.filter(r => r.status === 'rejected' || (typeof r.value === 'string' && r.value.includes('ERROR')));";
let renderErrorsRep = const errorOps = results.filter(r => r.status === 'rejected' || (typeof r.value === 'string' && r.value.includes('ERROR')));
                          const errorStr = JSON.stringify(errorOps);
                          if (errorOps.length > 0 && errorStr === lastErrorOpsStr) {
                              identicalErrorCount++;
                          } else {
                              identicalErrorCount = 0;
                              lastErrorOpsStr = errorStr;
                          };
c = c.replace(renderErrors, renderErrorsRep);

let limitCheck = "const hitLimit = (typeof loop_i !== 'undefined') && loop_i >= MAX_LOOPS - 1;";
let limitCheckRep = "const hitLimit = (typeof loop_i !== 'undefined') && (loop_i >= MAX_LOOPS - 1 || identicalErrorCount > 4);";
c = c.replace(limitCheck, limitCheckRep);

let statusLineL = "statusLine = \\\\\\\n\\\\n> ?? **Se alcanzó el límite de \ pasos** — la tarea puede estar incompleta\\\;";
let statusLineLRep = "statusLine = identicalErrorCount > 4 ? \\\\\\\n\\\\n> ?? **Confinamiento de Error (Anti-Stuck)** — 4 repeticiones idénticas abortadas\\\ : \\\\\\\n\\\\n> ?? **Se alcanzó el límite de \ pasos** — la tarea puede estar incompleta\\\;";
c = c.replace(statusLineL, statusLineLRep);

fs.writeFileSync('src/routes/+page.svelte', c);
console.log('Script end');
