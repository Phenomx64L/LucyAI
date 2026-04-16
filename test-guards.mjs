// Standalone test of normalizeCommand + analyzeCommand + isDestructiveCmd.
// Mirrors production logic from command-guard.ts and +page.svelte.

function normalizeCommand(cmd) {
    let s = String(cmd || '');
    s = s.replace(/`([^\r\n])/g, '$1');
    s = s.replace(/\^([^\r\n])/g, '$1');
    for (let i = 0; i < 6; i++) {
        const before = s;
        s = s.replace(/(['"])([^'"`]*)\1\s*\+\s*(['"])([^'"`]*)\3/g, (_m, q1, a, _q2, b) => `${q1}${a}${b}${q1}`);
        if (s === before) break;
    }
    const envMap = {
        systemroot: 'C:\\Windows', windir: 'C:\\Windows', systemdrive: 'C:',
        programfiles: 'C:\\Program Files', 'programfiles(x86)': 'C:\\Program Files (x86)',
        programdata: 'C:\\ProgramData', allusersprofile: 'C:\\ProgramData',
        public: 'C:\\Users\\Public', temp: 'C:\\Windows\\Temp', tmp: 'C:\\Windows\\Temp',
        userprofile: 'C:\\Users\\User', localappdata: 'C:\\Users\\User\\AppData\\Local',
        appdata: 'C:\\Users\\User\\AppData\\Roaming',
    };
    const expand = n => envMap[n.toLowerCase()] ?? `%${n}%`;
    s = s.replace(/%([A-Za-z_][A-Za-z0-9_()]*)%/g, (_m, n) => expand(n));
    s = s.replace(/\$\{?env:([A-Za-z_][A-Za-z0-9_()]*)\}?/gi, (_m, n) => expand(n));
    try { s = s.normalize('NFKC'); } catch {}
    s = s.replace(/\\{2,}/g, '\\');
    s = s.replace(/(['"])(C:\\[^'"]*)\1/gi, '$2');
    return s;
}

const _DESTRUCTIVE_RE = /(?:netsh\s+interface|Set-NetAdapter|Remove-|Stop-Service|Disable-|Set-Service|reg\s+(?:delete|add)\b|net\s+(?:stop|user|group|localgroup)|Clear-EventLog|wevtutil\s+(?:cl|clear-log)\b|Restart-Computer|Stop-Computer|Enable-PSRemoting|Set-ExecutionPolicy|Format-Volume|Initialize-Disk|(?:C:\\Windows\\System32|System32\\\\?))/i;
const isDestructiveCmd = (cmd) => _DESTRUCTIVE_RE.test(cmd) || _DESTRUCTIVE_RE.test(normalizeCommand(cmd));

const WIN_RE = [
    /\bRemove-Item\s+.*-Recurse\b.*-Force\b/i,
    /\bRemove-Item\s+.*(?:C:\\Windows|C:\\Program\s*Files|System32)/i,
    /\bStop-Service\s+.*(?:WinRM|Dnscache|wuauserv|MpsSvc|EventLog)/i,
    /\bClear-EventLog\b/i,
];
const matchAny = (cmd) => {
    const norm = normalizeCommand(cmd);
    return WIN_RE.some(r => r.test(cmd) || r.test(norm));
};

let pass = 0, fail = 0;
const t = (name, actual, expected) => {
    const ok = actual === expected;
    if (ok) { pass++; console.log(`  PASS  ${name}`); }
    else    { fail++; console.log(`  FAIL  ${name}\n        expected: ${JSON.stringify(expected)}\n        actual:   ${JSON.stringify(actual)}`); }
};

console.log('\n== Normalization ==');
t('PS backtick',
    normalizeCommand("Remove-Item C:\\Windows\\Sys`tem32\\drivers"),
    'Remove-Item C:\\Windows\\System32\\drivers');
t('PS concat 2-part (then unquoted)',
    normalizeCommand("Remove-Item ('C:\\Sys' + 'tem32\\hosts')"),
    "Remove-Item (C:\\System32\\hosts)");
t('PS concat 3-part (then unquoted)',
    normalizeCommand("$p = 'C:\\Win' + 'dows\\Sys' + 'tem32'"),
    "$p = C:\\Windows\\System32");
t('%SystemRoot%',
    normalizeCommand("del %SystemRoot%\\System32\\config\\sam"),
    'del C:\\Windows\\System32\\config\\sam');
t('$env:SystemRoot',
    normalizeCommand("Remove-Item $env:SystemRoot\\System32"),
    'Remove-Item C:\\Windows\\System32');
t('${env:WinDir}',
    normalizeCommand("rm ${env:WinDir}\\notepad.exe"),
    'rm C:\\Windows\\notepad.exe');
t('cmd caret',
    normalizeCommand('del C:\\Win^dows\\System32'),
    'del C:\\Windows\\System32');
t('quoted path unwrap',
    normalizeCommand('Remove-Item "C:\\Windows\\System32"'),
    'Remove-Item C:\\Windows\\System32');

console.log('\n== isDestructiveCmd: RT-1 System32 obfuscation ==');
t('plain System32',                isDestructiveCmd('Remove-Item C:\\Windows\\System32\\foo'), true);
t('PS concat',                     isDestructiveCmd("Remove-Item ('C:\\Windows\\Sys' + 'tem32\\drivers')"), true);
t('backtick',                      isDestructiveCmd('Remove-Item C:\\Windows\\Sys`tem32\\foo'), true);
t('%SystemRoot%',                  isDestructiveCmd('del %SystemRoot%\\System32\\config\\sam'), true);
t('$env:WinDir',                   isDestructiveCmd('Remove-Item $env:WinDir\\System32\\foo'), true);
t('benign Get-Process',            isDestructiveCmd('Get-Process | Where CPU -gt 100'), false);
t('benign Get-ChildItem',          isDestructiveCmd('Get-ChildItem C:\\Users -Recurse'), false);

console.log('\n== isDestructiveCmd: RT-2 audit cleanup ==');
t('Clear-EventLog',                isDestructiveCmd('Clear-EventLog -LogName Security'), true);
t('wevtutil cl',                   isDestructiveCmd('wevtutil cl Security'), true);
t('wevtutil clear-log',            isDestructiveCmd('wevtutil clear-log Security'), true);
t('benign wevtutil epl (export)',  isDestructiveCmd('wevtutil epl Security C:\\backup\\sec.evtx'), false);

console.log('\n== Pattern matchAny ==');
t('Remove-Item -Recurse -Force',   matchAny('Remove-Item -Recurse -Force C:\\temp'), true);
t('Stop-Service WinRM',            matchAny('Stop-Service -Name WinRM -Force'), true);
t('Clear-EventLog',                matchAny('Clear-EventLog Security'), true);
t('System32 via concat',           matchAny("Remove-Item ('C:\\Windows\\Sys' + 'tem32')"), true);
t('benign ls',                     matchAny('Get-ChildItem C:\\Users'), false);

console.log('\n== Adversarial bypass attempts ==');
// Things a smart attacker (or smart LLM) would actually try.
t('mixed case env var',            isDestructiveCmd('del %SYSTEMROOT%\\System32\\foo'), true);
t('mixed case PS env',             isDestructiveCmd('rm $ENV:SystemRoot\\System32'), true);
t('Stop-Service via concat',       isDestructiveCmd("Stop-Serv'+'ice -Name WinRM"), false); // KNOWN GAP: cmdlet name split
t('Format-Volume',                 isDestructiveCmd('Format-Volume -DriveLetter D'), true);
t('reg delete HKLM',               isDestructiveCmd('reg delete HKLM\\SYSTEM\\CCS\\Services\\foo'), true);
t('net user delete',               isDestructiveCmd('net user attacker /delete'), true);
t('Disable-WindowsOptionalFeature',isDestructiveCmd('Disable-WindowsOptionalFeature -Online -FeatureName SMB1'), true);
t('Set-ExecutionPolicy bypass',    isDestructiveCmd('Set-ExecutionPolicy Bypass -Scope Process'), true);
t('mixed obfuscation: backtick + concat + env',
    isDestructiveCmd("Remove-Item ($env:Sys`temRoot + '\\Sys' + 'tem32')"), true);
t('lowercase system32',            isDestructiveCmd('del C:\\windows\\system32\\config'), true);

console.log('\n== Anti-loop hash ==');
const counts = new Map();
const MAX = 3;
const toolHash = (kind, args) => `${kind}::${String(args).trim().toLowerCase().replace(/\s+/g, ' ').slice(0, 400)}`;
const checkLoop = (kind, args) => {
    const h = toolHash(kind, args);
    const prev = counts.get(h) || 0;
    counts.set(h, prev + 1);
    return prev >= MAX;
};
t('readfile call 1',               checkLoop('readfile', 'C:\\foo.ps1'), false);
t('readfile call 2',               checkLoop('readfile', 'C:\\foo.ps1'), false);
t('readfile call 3',               checkLoop('readfile', 'C:\\foo.ps1'), false);
t('readfile call 4 BLOCKED',       checkLoop('readfile', 'C:\\foo.ps1'), true);
t('different file allowed',        checkLoop('readfile', 'C:\\bar.ps1'), false);
t('whitespace-insensitive',        checkLoop('readfile', '  C:\\FOO.ps1  '), true);
t('different kind fresh',          checkLoop('execute:powershell', 'Get-Service'), false);

console.log(`\n========================\n  ${pass} passed, ${fail} failed\n========================`);
process.exit(fail > 0 ? 1 : 0);
