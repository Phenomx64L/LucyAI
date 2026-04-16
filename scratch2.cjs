const fs = require('fs');

const path = 'X:\\Rust_Projects\\lucy-svelte\\src\\routes\\+page.svelte';
let content = fs.readFileSync(path, 'utf8');

// Add backgroundTasks
if (!content.includes('let backgroundTasks = {};')) {
    content = content.replace('let pendingLearnSpeak  = false;', "let pendingLearnSpeak  = false;\n    let backgroundTasks = {};");
}

// Add to regex
content = content.replace(/mcp_query\):\/i/g, "mcp_query|fork_task|wait_task):/i");

// Add parsing logic
const target = "for (const mcpQ of [...agentResp.matchAll(/<TOOL>mcp_query";
const injection = `for (const fork of [...agentResp.matchAll(/<TOOL>fork_task:([^|]+)\\|\\|\\|([\\s\\S]*?)<\\/TOOL>/gi)]) {
    toolUsed = true;
    lucyText = lucyText.replace(/<TOOL>fork_task:[\\s\\S]*?<\\/TOOL>/gi, '');
    const tid=fork[1].trim(); const inst=fork[2].trim();
    backgroundTasks[tid] = invoke('ask_lucy', {prompt: \`[SUB-AGENT TASK]: \${inst}\`, context: ctx.substring(Math.max(0, ctx.length - 8000)), userName: lucyConfig.name, runbooksDir: lucyConfig.runbooksDir, model: 'gemini-2.5-flash-lite-preview', images: null, lang: userLang, hostsJson: JSON.stringify($hosts)});
    stepsHtml += \`\\n<span style='font-size:11px;color:#88a;'><br>[🧠 Sub-Agent Forked: \${tid}]</span>\`;
}
for (const w of [...agentResp.matchAll(/<TOOL>wait_task:([^<]+)<\\/TOOL>/gi)]) {
    toolUsed = true;
    lucyText = lucyText.replace(/<TOOL>wait_task:[^<]+<\\/TOOL>/gi, '');
    const tid=w[1].trim();
    if(backgroundTasks[tid]){
        readOnlyTasks.push({ label: \`[⏱️ Wait Task: \${tid}]\`, fn: () => backgroundTasks[tid].then(c => \`[SUB-AGENT \${tid} RESULT]\\n\`+c).catch(e => \`[SUB-AGENT \${tid} ERROR]\\n\`+e) });
    }
}
`;

if (!content.includes('fork_task:([^|]+)')) {
    content = content.replace(target, injection + target);
}

fs.writeFileSync(path, content, 'utf8');
