import fs from 'fs';
let s = fs.readFileSync('src/routes/+page.svelte', 'utf8');

// 1. MAX_LOOPS = 100
s = s.replace('const MAX_LOOPS = 15;',
    "const MAX_LOOPS = 100;\n                let identicalErrorCount = 0;\n                let lastErrorOpsStr = '';\n                let lastThoughtText = '';");

// 2. Anti-stuck: hitLimit
s = s.replace(
    "const hitLimit = (typeof loop_i !== 'undefined') && loop_i >= MAX_LOOPS - 1;",
    "const hitLimit = (typeof loop_i !== 'undefined') && (loop_i >= MAX_LOOPS - 1 || identicalErrorCount > 4);"
);

// 3. Increment counter after results
const errLine = "const errorOps = results.filter(r => r.status === 'rejected' || (typeof r.value === 'string' && r.value.includes('ERROR')));";
const errRep   = errLine + "\n                          const errorStr = JSON.stringify(errorOps);\n                          if (errorOps.length > 0 && errorStr === lastErrorOpsStr) identicalErrorCount++;\n                          else { identicalErrorCount = 0; lastErrorOpsStr = errorStr; }";
if (!s.includes('identicalErrorCount++')) s = s.replace(errLine, errRep);

// 4. Thought dedup
const thOld = "if (thM) {\n                          thoughtsAccum += thM[1].trim() + '\\n\\n';";
const thNew  = "if (thM) {\n                          const rawT = thM[1].trim();\n                          if (rawT === lastThoughtText) { thoughtsAccum += '\\n[Pensamiento repetido... omitido]\\n'; }\n                          else { thoughtsAccum += rawT + '\\n\\n'; lastThoughtText = rawT; }";
if (s.includes(thOld)) s = s.replace(thOld, thNew);

fs.writeFileSync('src/routes/+page.svelte', s);
console.log('MAX_LOOPS:', s.includes('const MAX_LOOPS = 100') ? '100 OK' : 'FAIL');
console.log('Anti-Stuck:', s.includes('identicalErrorCount++') ? 'OK' : 'FAIL');
console.log('Thought dedup:', s.includes('lastThoughtText') ? 'OK' : 'FAIL');
