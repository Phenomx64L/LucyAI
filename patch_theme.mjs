import fs from 'fs';
let s = fs.readFileSync('src/routes/+page.svelte', 'utf8');

// ── 1. Add activeTheme state variable after darkMode ──────────────────────
const darkModeLine = `let darkMode           = localStorage?.getItem('lucy_dark') !== 'false'; // tema oscuro/claro`;
const darkModeRep  = `let darkMode           = localStorage?.getItem('lucy_dark') !== 'false'; // tema oscuro/claro
    let activeTheme        = localStorage?.getItem('lucy_theme') || 'default'; // warp themes: default|ocean|hacker`;
if (!s.includes('activeTheme')) {
    s = s.replace(darkModeLine, darkModeRep);
}

// ── 2. Add setTheme reactive call inside onMount ──────────────────────────
const onMountStr = 'onMount(async () => {';
const setThemeBlock = `onMount(async () => {
        // Apply Warp-style theme on boot
        document.documentElement.setAttribute('data-theme', activeTheme);`;
if (!s.includes("setAttribute('data-theme'") && s.includes(onMountStr)) {
    s = s.replace(onMountStr, setThemeBlock);
}

// ── 3. Add setTheme function near toggleTheme ─────────────────────────────
const toggleThemeFn = `function toggleTheme() {`;
const setThemeFn = `function setTheme(t) {
        activeTheme = t;
        localStorage.setItem('lucy_theme', t);
        document.documentElement.setAttribute('data-theme', t);
    }
    function toggleTheme() {`;
if (!s.includes('function setTheme') && s.includes(toggleThemeFn)) {
    s = s.replace(toggleThemeFn, setThemeFn);
}

// ── 4. Add sidebar-glass class to <aside> ────────────────────────────────
const asideOld = `<aside class="sidebar" class:open={!sidebarCollapsed} class:closed={sidebarCollapsed}`;
const asideNew = `<aside class="sidebar sidebar-glass" class:open={!sidebarCollapsed} class:closed={sidebarCollapsed}`;
if (!s.includes('sidebar-glass') && s.includes(asideOld)) {
    s = s.replace(asideOld, asideNew);
}

// ── 5. Add theme-picker dots just before </aside> ─────────────────────────
const asideClose = '</aside>';
const themePicker = `
      <!-- ── Warp-style theme picker ── -->
      <div class="theme-picker">
        <button class="theme-dot theme-dot-default" class:active={activeTheme==='default'} on:click={() => setTheme('default')} title="Tema Default" aria-label="Tema Default"></button>
        <button class="theme-dot theme-dot-ocean"   class:active={activeTheme==='ocean'}   on:click={() => setTheme('ocean')}   title="Tema Ocean"   aria-label="Tema Ocean"></button>
        <button class="theme-dot theme-dot-hacker"  class:active={activeTheme==='hacker'}  on:click={() => setTheme('hacker')}  title="Tema Hacker"  aria-label="Tema Hacker"></button>
      </div>
    </aside>`;
if (!s.includes('theme-picker') && s.includes(asideClose)) {
    // Replace first occurrence of </aside>
    s = s.replace(asideClose, themePicker);
}

// ── 6. Add bg-warp-gradient to the root .body div ────────────────────────
const bodyOld = 'class="body" class:focus-mode={focusMode}>';
const bodyNew = 'class="body bg-warp-gradient" class:focus-mode={focusMode} data-theme={activeTheme}>';
if (!s.includes('bg-warp-gradient') && s.includes(bodyOld)) {
    s = s.replace(bodyOld, bodyNew);
}

fs.writeFileSync('src/routes/+page.svelte', s);
console.log('Theme patch v1 applied');
console.log('activeTheme:', s.includes('activeTheme') ? 'OK' : 'MISSING');
console.log('setTheme:', s.includes('function setTheme') ? 'OK' : 'MISSING');
console.log('sidebar-glass:', s.includes('sidebar-glass') ? 'OK' : 'MISSING');
console.log('theme-picker:', s.includes('theme-picker') ? 'OK' : 'MISSING');
console.log('bg-warp-gradient:', s.includes('bg-warp-gradient') ? 'OK' : 'MISSING');
