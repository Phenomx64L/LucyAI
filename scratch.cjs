const fs = require('fs');

const path = 'X:\\Rust_Projects\\lucy-svelte\\src\\routes\\+page.svelte';
let content = fs.readFileSync(path, 'utf8');

function replaceMatchWithMatchAll(str, varName, toolRegexString) {
    // Escape backslashes for exact matching in the file.
    // The literal in file is: /<TOOL>cd:([^<]+)<\/TOOL>/i
    // We pass toolRegexString as exactly what we want to match between the parentheses of .match()
    const regexPattern = toolRegexString.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

    const searchPattern = new RegExp(
        `const\\s+${varName}\\s*=\\s*agentResp\\.match\\(${regexPattern}\\);\\s*if\\s*\\(${varName}\\)\\s*\\{`,
        'g'
    );
    
    let count = 0;
    const result = str.replace(searchPattern, (match) => {
        count++;
        // Reconstruct global regex natively
        const globalRegex = toolRegexString.replace('/i', '/gi');
        return `for (const ${varName} of [...agentResp.matchAll(${globalRegex})]) {`;
    });
    console.log(`Replaced ${varName}: ${count}`);
    return result;
}

content = replaceMatchWithMatchAll(content, 'cdM', '/<TOOL>cd:([^<]+)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'sfM', '/<TOOL>searchfiles:([\\s\\S]+?)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'lfM', '/<TOOL>locate_file:([^<]+)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'idxM', '/<TOOL>start_indexer:([^<]+)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'diffM', '/<TOOL>system_diff:([^<]+)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'rbM', '/<TOOL>search_runbooks:([^<]+)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'rfM', '/<TOOL>readfile:([^<]+)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'rlM', '/<TOOL>readlines:([^<:]+):(\\d+):(\\d+)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'ldM', '/<TOOL>listdir:([^<]+)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'acAST', '/<TOOL>analyze_code:([^<]+)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'evtM', '/<TOOL>eventlog:([^<:]+):(\\d+)(?::([^<]+))?<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'regM', '/<TOOL>registry:([^|<]+)\\|([^|<]+)\\|([^<]*)<\\/TOOL>/i');
content = replaceMatchWithMatchAll(content, 'efM', '/<TOOL>editfile:([\\s\\S]+?)<\\/TOOL>/i');

console.log('Writing back...');
fs.writeFileSync(path, content, 'utf8');
