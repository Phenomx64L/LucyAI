<script lang="ts">
    import { createEventDispatcher } from 'svelte';
    // Inlined base64 data URL — bypasses Tauri 2.x webview path resolution
    // edge cases on Windows (`https://tauri.localhost` scheme + Vite-hashed
    // asset chunks didn't render reliably). The image lives inside the JS
    // bundle so no network fetch, no path resolution, no cache surprises.
    import { LUCY_AVATAR_DATA_URL as lucyAvatarUrl } from '$lib/assets/lucy-avatar-data';
    import { getTabRevStore } from '$lib/page/tabs-store';
    import AgentChapterView from '$lib/AgentChapterView.svelte';

    export let tab: any;
    export let isEN: boolean = false;
    export let chatSearch: string = '';
    export let isActiveTab: boolean = false;
    // Optional data: URL or http URL pointing to the user's profile picture.
    // When set, replaces the initials avatar with an <img>. Persisted in
    // lucyConfig by the parent and forwarded here.
    export let userAvatarUrl: string = '';
    // Fallback when a message lacks `rawRole` — used by the initials avatar
    // so it never has to show '?'. Comes from lucyConfig.name in the parent.
    export let userName: string = '';

    // ── P2 audit: granular per-tab reactivity ──
    // When the parent calls `refreshSoft(tab.id)`, only THIS tab's rev
    // store fires. We subscribe via `$revStore` so this component re-runs
    // its template, but cousin tabs (other ChatThreads) don't notice.
    //
    // We read `$revStore` inside `visibleMessages()` and the iteration
    // below so Svelte tracks it as a dependency. The actual value is
    // discarded — it's just a "tick" signal.
    $: revStore = tab?.id ? getTabRevStore(tab.id) : null;
    $: tabRev = revStore ? $revStore : 0;

    const dispatch = createEventDispatcher<{
        pinmessage: { msg: any };
        buttonaction: { msg: any; event: MouseEvent };
        togglereasoning: { msg: any };
        codeclick: { path: string };
        fixclick: { key: string };
    }>();

    function handleAreaClick(e: MouseEvent) {
        const target = e.target as HTMLElement;
        const codeBtn = target.closest('.lucy-code-btn') as HTMLElement | null;
        if (codeBtn?.dataset.path) {
            dispatch('codeclick', { path: codeBtn.dataset.path });
        }
        const fixBtn = target.closest('.lucy-fix-btn') as HTMLElement | null;
        if (fixBtn) {
            const key = fixBtn.dataset.fixKey;
            if (key) dispatch('fixclick', { key });
        }
    }

    function visibleMessages(messages: any[]) {
        return messages.filter(m =>
            m.role !== 'hidden' && (
                !chatSearch || !isActiveTab ||
                m.role === 'system' || m.role === 'thinking' || m.role === 'streaming' ||
                (m.rawContent || '').toLowerCase().includes(chatSearch.toLowerCase())
            )
        );
    }

    /**
     * Compute 1–2 character initials for the user avatar from a name string.
     * "Iván López" → "IL", "Luna" → "LU", "" → "?".
     */
    function getInitials(name: string | undefined | null): string {
        if (!name) return '?';
        const trimmed = name.trim();
        if (!trimmed) return '?';
        const parts = trimmed.split(/\s+/);
        if (parts.length >= 2) {
            return (parts[0][0] + parts[1][0]).toUpperCase();
        }
        return trimmed.slice(0, 2).toUpperCase();
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="chat-area" on:click={handleAreaClick}>
    {#each (void tabRev, visibleMessages(tab.messages)) as msg (msg.id)}
        {#if msg.role === 'thinking'}
            <div class="msg-thinking">
                <div class="thinking-dots">
                    <span></span><span></span><span></span>
                </div>
                <span class="thinking-label">Lucy está procesando...</span>
            </div>
        {:else if msg.role === 'reasoning'}
            <div class="msg-reasoning {msg.active ? 'reasoning-active' : 'reasoning-done'} {msg.collapsed ? 'reasoning-collapsed' : ''}">
                <button type="button" class="reasoning-header"
                    on:click={() => dispatch('togglereasoning', { msg })}>
                    <span class="reasoning-icon">·</span>
                    <span class="reasoning-title">
                        {#if msg.active}Pensando…{:else}Pensó durante {msg.duration.toFixed(1)}s{/if}
                    </span>
                    {#if msg.active}
                        <span class="reasoning-timer">{msg.duration.toFixed(1)}s</span>
                    {/if}
                    <span class="reasoning-chevron">{msg.collapsed ? '▸' : '▾'}</span>
                </button>
                {#if !msg.collapsed && msg.html}
                    <div class="reasoning-body">{@html msg.html}</div>
                {/if}
            </div>
        {:else if msg.role === 'streaming' && !msg.rawContent}
            <div class="msg-lucy msg-skel msg-enter">
                <span class="lucy-avatar-wrap" title="Lucy · v1.4.0">
                    <img class="lucy-avatar" src={lucyAvatarUrl} alt="Lucy" />
                    <span class="lucy-status processing" aria-label="Lucy está pensando"></span>
                </span>
                <div class="mn">Lucy</div>
                <div class="skel-block">
                    <div class="skel-line" style="width:84%"></div>
                    <div class="skel-line" style="width:68%"></div>
                    <div class="skel-line" style="width:91%"></div>
                </div>
                <div class="skel-block" style="margin-top:7px">
                    <div class="skel-line" style="width:52%"></div>
                </div>
            </div>
        {:else}
            <div class="{msg.role === 'user' ? 'msg-user' : msg.role === 'system' ? 'sys-msg' : 'msg-lucy'}{msg.role === 'streaming' ? ' streaming-active' : ''}{msg.pinned ? ' msg-pinned' : ''} msg-enter"
                style={msg.style || ''}>
                {#if msg.role === 'lucy' || msg.role === 'streaming'}
                    <span class="lucy-avatar-wrap" title="Lucy · v1.4.0">
                        <img class="lucy-avatar" src={lucyAvatarUrl} alt="Lucy" />
                        <span class="lucy-status {tab?.isProcessing && msg.role === 'streaming' ? 'processing' : 'active'}"
                              aria-label={tab?.isProcessing && msg.role === 'streaming' ? 'Procesando' : 'Activo'}></span>
                    </span>
                {/if}
                {#if msg.role === 'user'}
                    {#if userAvatarUrl}
                        <span class="user-avatar user-avatar-img" title={msg.rawRole || userName || 'Usuario'} aria-label={msg.rawRole || userName || 'Usuario'}>
                            <img src={userAvatarUrl} alt={msg.rawRole || userName || 'Usuario'} />
                        </span>
                    {:else}
                        <span class="user-avatar" title={msg.rawRole || userName || 'Usuario'} aria-label={msg.rawRole || userName || 'Usuario'}>
                            {getInitials(msg.rawRole || userName)}
                        </span>
                    {/if}
                {/if}
                {#if msg.role !== 'system'}
                    {#if msg.rawRole && (msg.role === 'user' || msg.role === 'lucy')}
                        <button class="msg-pin" class:on={msg.pinned}
                            title={msg.pinned ? (isEN ? 'Unpin' : 'Quitar pin') : (isEN ? 'Pin to context' : 'Fijar al contexto')}
                            on:click={() => dispatch('pinmessage', { msg })}>·</button>
                    {/if}
                    <!-- U3 — Chapter view for multi-step agent tasks -->
                    {#if msg.chapterData && msg.viewMode !== 'linear'}
                        <div class="mn">Lucy <span style="font-size:10px; opacity:0.6">(Agent · Chapter)</span></div>
                        <AgentChapterView
                            title={msg.chapterData.title}
                            objective={msg.chapterData.objective}
                            elapsedMs={msg.chapterData.elapsedMs}
                            steps={msg.chapterData.steps}
                            finalHtml={msg.chapterData.finalHtml}
                            {isEN}
                            on:flip={() => { msg.viewMode = 'linear'; tab.messages = [...tab.messages]; }} />
                    {:else if msg.chapterData && msg.viewMode === 'linear'}
                        <button class="chap-flip-back"
                                on:click={() => { msg.viewMode = 'chapter'; tab.messages = [...tab.messages]; }}
                                title={isEN ? 'Switch to chapter view' : 'Cambiar a vista de capítulos'}>
                            ◆ {isEN ? 'Chapter view' : 'Vista de capítulos'}
                        </button>
                        {@html msg.html}
                    {:else}
                        {@html msg.html}
                    {/if}
                    {#if msg.attachments?.length}
                        <div class="msg-img-gallery">
                            {#each msg.attachments as att}
                                <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                                <figure class="msg-img-fig" title={att.name} on:click={() => { const w=window.open('','_blank'); if(w){w.document.write(`<img src="${att.previewUrl}" style="max-width:100%;max-height:100vh;">`);} }} on:keydown>
                                    <img src={att.previewUrl} alt={att.name} class="msg-img-thumb" />
                                    <figcaption class="msg-img-cap">{att.name}</figcaption>
                                </figure>
                            {/each}
                        </div>
                    {/if}
                    {#if msg.time}<div class="msg-time">{msg.time}</div>{/if}
                {:else}
                    {@html msg.html}
                {/if}
                {#if msg.button}
                    <button class="msg-btn"
                        on:click={(e) => dispatch('buttonaction', { msg, event: e })}>
                        {msg.button.text}
                    </button>
                {/if}
            </div>
        {/if}
    {/each}
</div>

<style>
    :global(.chat-wrap){display:none;flex-direction:column;flex:1;overflow:hidden;max-width:960px;width:100%;margin:0 auto;}
    :global(.chat-wrap.on){display:flex;}
    :global(.chat-area){flex:1;overflow-y:auto;padding:16px 22px;display:flex;flex-direction:column;gap:10px;min-height:0;}
    :global(.msg-user),:global(.msg-lucy),:global(.sys-msg),:global(.msg-thinking){content-visibility:auto;contain-intrinsic-size:0 80px;}
    /* ── Bubble gradient backgrounds — subtle depth, no distraction ──────
     * msg-user: diagonal blue→deeper blue. Bubble feels "lit" from upper-left.
     * msg-lucy: diagonal turquoise→teal with low opacity over a dark base —
     *           echoes the avatar's color palette.
     * The gradient angle (135deg) matches the avatar's gradient direction
     * so the whole interface has visual continuity.
     */
    :global(.msg-img-gallery){display:flex;flex-wrap:wrap;gap:8px;margin-top:8px;}
    :global(.msg-img-fig){margin:0;cursor:zoom-in;border-radius:6px;overflow:hidden;border:1px solid rgba(255,255,255,0.08);transition:transform 0.15s,box-shadow 0.15s;}
    :global(.msg-img-fig:hover){transform:scale(1.03);box-shadow:0 4px 20px rgba(0,0,0,0.5);}
    :global(.msg-img-thumb){display:block;max-width:260px;max-height:180px;object-fit:contain;background:rgba(0,0,0,0.2);}
    :global(.msg-img-cap){display:block;font-size:10px;color:#64748b;padding:3px 6px;background:rgba(0,0,0,0.3);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:260px;}
    :global(.msg-user){
        align-self:flex-end;
        background:linear-gradient(135deg,#22283a 0%,#1c2030 60%,#191c28 100%);
        border:1px solid rgba(96,165,250,.10);
        border-right:2px solid var(--blue);
        border-radius:10px 10px 0 10px;
        padding:10px 14px;
        max-width:78%;
        white-space:pre-wrap;
        box-shadow:inset 0 1px 0 rgba(96,165,250,.05),0 2px 12px -6px rgba(0,0,0,.4);
    }
    :global(.msg-lucy){
        align-self:flex-start;
        background:linear-gradient(135deg,rgba(16,185,129,.09) 0%,rgba(16,185,129,.04) 55%,rgba(16,185,129,.02) 100%),rgba(20,28,38,.35);
        border:1px solid rgba(16,185,129,.12);
        border-left:2px solid #10b981;
        border-radius:0 10px 10px 10px;
        padding:10px 14px;
        max-width:88%;
        line-height:1.6;
        box-shadow:inset 0 1px 0 rgba(16,185,129,.06),0 2px 12px -6px rgba(0,0,0,.4);
    }
    /* ── Avatar wrap — positioning context for the status indicator dot ────
     * Floats left so the bubble text wraps around it. The .lucy-avatar
     * itself is display:block inside this wrap; the .lucy-status sits
     * absolutely on the bottom-right corner.
     */
    :global(.lucy-avatar-wrap){
        position:relative !important;
        display:block !important;
        float:left;
        margin:0 12px 4px 0;
        flex-shrink:0;
    }
    :global(.lucy-avatar){
        display:block !important;
        width:38px !important;
        height:38px !important;
        object-fit:cover;
        border-radius:8px;
        border:1px solid rgba(16,185,129,.45);
        box-shadow:0 0 0 1px rgba(0,0,0,.35),0 0 14px -2px rgba(16,185,129,.32);
        background:rgba(16,185,129,0.06); /* visible fallback if image fails */
        transition:transform .25s ease,box-shadow .25s ease,border-color .25s ease;
        cursor:default;
        user-select:none;
        -webkit-user-drag:none;
    }
    /* Status indicator dot — Slack/Discord style presence light */
    :global(.lucy-status){
        position:absolute !important;
        bottom:-2px;
        right:-2px;
        width:11px;
        height:11px;
        border-radius:50%;
        background:#10b981;                /* green = active by default */
        border:2px solid var(--bg2,#060a0f);
        box-shadow:0 0 6px rgba(16,185,129,.6);
        pointer-events:none;
        z-index:2;
    }
    :global(.lucy-status.processing){
        background:#3b9eff;
        box-shadow:0 0 6px rgba(59,158,255,.6);
        animation:lucyStatusPulse 1.2s ease-in-out infinite;
    }
    @keyframes lucyStatusPulse{
        0%,100%{box-shadow:0 0 6px rgba(59,158,255,.6); transform:scale(1);}
        50%    {box-shadow:0 0 14px rgba(59,158,255,1); transform:scale(1.2);}
    }
    /* Pulsing avatar border while Lucy is streaming a response */
    :global(.msg-lucy.streaming-active) :global(.lucy-avatar),
    :global(.msg-lucy.msg-skel) :global(.lucy-avatar){
        animation:lucyAvatarPulse 1.8s ease-in-out infinite;
    }
    @keyframes lucyAvatarPulse{
        0%,100%{box-shadow:0 0 0 1px rgba(0,0,0,.35),0 0 14px -2px rgba(16,185,129,.25);}
        50%    {box-shadow:0 0 0 1px rgba(0,0,0,.35),0 0 22px 1px rgba(16,185,129,.55);}
    }
    /* Hover effect — applied to the wrap so the status dot scales together */
    :global(.lucy-avatar-wrap:hover) :global(.lucy-avatar){
        transform:scale(1.12) translateY(-1px);
        border-color:rgba(16,185,129,.65);
        box-shadow:0 0 0 1px rgba(0,0,0,.4),0 0 24px 2px rgba(16,185,129,.55),0 6px 18px -4px rgba(16,185,129,.4);
    }
    /* ── User avatar — initials in a tinted circle (matches Luna's blue) ─── */
    :global(.user-avatar){
        display:block !important;
        float:right;
        width:38px !important;
        height:38px !important;
        margin:0 0 4px 12px;
        border-radius:8px;
        background:linear-gradient(135deg,#3b9eff 0%,#2563eb 100%);
        color:#fff;
        font-family:var(--mono,ui-monospace,monospace);
        font-size:13px;
        font-weight:700;
        letter-spacing:.5px;
        text-align:center;
        line-height:38px;
        border:1px solid rgba(59,158,255,.45);
        box-shadow:0 0 0 1px rgba(0,0,0,.35),0 0 14px -2px rgba(59,158,255,.32);
        user-select:none;
        flex-shrink:0;
        transition:transform .25s ease,box-shadow .25s ease;
        cursor:default;
    }
    :global(.user-avatar:hover){
        transform:scale(1.12) translateY(-1px);
        box-shadow:0 0 0 1px rgba(0,0,0,.4),0 0 24px 2px rgba(59,158,255,.55),0 6px 18px -4px rgba(59,158,255,.4);
    }
    /* Image variant — the gradient background is hidden behind the photo.
       padding:0 so the image fills the circle. */
    :global(.user-avatar-img){
        padding:0;
        overflow:hidden;
    }
    :global(.user-avatar-img > img){
        width:100%;height:100%;
        object-fit:cover;
        display:block;
        border-radius:inherit;
    }
    /* On very narrow viewports, drop the avatars to keep room for text */
    @media (max-width:540px){
        :global(.lucy-avatar-wrap),:global(.user-avatar){display:none !important;}
    }
    /* ── Message entrance animation — slide-up + fade-in once on mount ──── */
    :global(.msg-enter){
        animation:msgSlideIn .28s cubic-bezier(.16,1,.3,1);
    }
    @keyframes msgSlideIn{
        from{opacity:0; transform:translateY(8px);}
        to  {opacity:1; transform:translateY(0);}
    }
    /* Honor reduced-motion preference */
    @media (prefers-reduced-motion:reduce){
        :global(.msg-enter){animation:none;}
        :global(.lucy-status.processing),
        :global(.msg-lucy.streaming-active) :global(.lucy-avatar),
        :global(.msg-lucy.msg-skel) :global(.lucy-avatar){animation:none;}
    }
    :global(.skel-block){display:flex;flex-direction:column;gap:7px;padding:4px 0;}
    :global(.skel-line){height:11px;border-radius:4px;background:linear-gradient(90deg,#0f1520 25%,#1e293b 50%,#0f1520 75%);background-size:200% 100%;animation:shimmer 1.6s ease-in-out infinite;}
    @keyframes shimmer{0%{background-position:200% 0;}100%{background-position:-200% 0;}}
    :global(.sys-msg){align-self:center;color:#334155;font-size:11px;font-style:italic;}
    :global(.mn){font-size:11px;font-weight:700;margin-bottom:5px;}
    :global(.msg-user .mn){color:var(--blue);}
    :global(.msg-lucy .mn){color:var(--acc);}
    :global(.msg-time){font-size:10px;color:#334155;text-align:right;margin-top:4px;}
    :global(.msg-btn){display:inline-flex;align-items:center;gap:5px;margin-top:10px;padding:6px 14px;border-radius:6px;font-size:12px;font-weight:600;cursor:pointer;border:1px solid rgba(255,170,0,.3);background:rgba(255,170,0,.1);color:var(--amber);transition:.15s;font-family:inherit;}
    :global(.msg-btn:hover){background:rgba(255,170,0,.2);}
    :global(.msg-btn:disabled){opacity:.4;cursor:not-allowed;}
    :global(.msg-lucy p){margin:0 0 6px;}:global(.msg-lucy p:last-child){margin-bottom:0;}
    :global(.msg-lucy pre){background:#0a0c15;border:1px solid var(--bdr);border-radius:7px;padding:10px 12px;overflow-x:auto;margin:8px 0;}
    :global(.msg-lucy code){font-family:var(--mono);font-size:12px;color:var(--acc);background:rgba(16,185,129,.06);padding:2px 5px;border-radius:3px;}
    :global(.msg-lucy pre code){color:#94a3b8;background:transparent;padding:0;border-radius:0;font-size:12px;}
    :global(.msg-lucy table){width:100%;border-collapse:collapse;margin:8px 0;font-size:12px;}
    :global(.msg-lucy th,.msg-lucy td){border:1px solid var(--bdr);padding:6px 10px;text-align:left;}
    :global(.msg-lucy th){background:var(--bg4);color:white;font-size:11px;font-weight:600;}
    :global(.msg-lucy ul,.msg-lucy ol){padding-left:18px;margin:6px 0;}
    :global(.msg-lucy li){margin-bottom:3px;}
    :global(.msg-lucy h1){font-size:15px;color:white;margin:10px 0 5px;font-weight:600;}
    :global(.msg-lucy h2){font-size:14px;color:white;margin:8px 0 4px;font-weight:600;}
    :global(.msg-lucy h3){font-size:13px;color:white;margin:6px 0 3px;font-weight:600;}
    :global(.msg-lucy strong){color:white;font-weight:600;}
    .msg-pin{position:absolute;top:6px;right:6px;background:transparent;border:none;color:var(--txt3);opacity:.35;cursor:pointer;font-size:12px;padding:2px 4px;border-radius:3px;transition:.15s;z-index:2;}
    .msg-pin:hover{opacity:1;background:rgba(167,139,250,.15);}
    .msg-pin.on{opacity:1;color:#fbbf24;text-shadow:0 0 6px rgba(251,191,36,.5);}
    :global(.msg-user),:global(.msg-lucy){position:relative;}
    :global(.msg-pinned){border-left:2px solid #fbbf24 !important;}
    .msg-thinking{display:flex;align-items:center;gap:10px;padding:10px 14px;align-self:flex-start;}
    .thinking-dots{display:flex;gap:4px;align-items:center;}
    .thinking-dots span{width:6px;height:6px;border-radius:50%;background:var(--acc);opacity:.4;animation:td .9s ease-in-out infinite;}
    .thinking-dots span:nth-child(2){animation-delay:.2s;}
    .thinking-dots span:nth-child(3){animation-delay:.4s;}
    @keyframes td{0%,100%{opacity:.4;transform:scale(1)}50%{opacity:1;transform:scale(1.3)}}
    .thinking-label{font-size:11px;color:#334155;font-style:italic;}
    .msg-reasoning{align-self:flex-start;max-width:88%;margin:6px 0 4px;border-radius:8px;background:rgba(167,139,250,.04);border:1px solid rgba(167,139,250,.14);border-left:2px solid transparent;overflow:hidden;transition:background .25s,border-color .25s;}
    .msg-reasoning.reasoning-active{background:radial-gradient(circle at 0% 50%,rgba(167,139,250,.18) 0%,transparent 25%),linear-gradient(110deg,rgba(167,139,250,.06) 0%,rgba(99,102,241,.14) 50%,rgba(167,139,250,.06) 100%),rgba(99,102,241,.04);background-size:100% 100%,250% 100%,100% 100%;background-repeat:no-repeat;animation:reasonShimmer 3.2s linear infinite,reasonGlow 4s ease-in-out infinite;border-left-color:#a78bfa;box-shadow:0 0 0 1px rgba(167,139,250,.10),0 4px 18px -8px rgba(99,102,241,.35),inset 0 0 30px -10px rgba(167,139,250,.08);position:relative;}
    .msg-reasoning.reasoning-active::before{content:'';position:absolute;inset:0;pointer-events:none;background:linear-gradient(110deg,transparent 45%,rgba(167,139,250,.08) 50%,transparent 55%);background-size:250% 100%;animation:reasonScan 2.4s ease-in-out infinite;}
    .msg-reasoning.reasoning-done{background:rgba(255,255,255,.015);border-left-color:rgba(167,139,250,.35);}
    @keyframes reasonShimmer{0%{background-position:0% 50%,0% 50%,0% 0%;}100%{background-position:0% 50%,250% 50%,0% 0%;}}
    @keyframes reasonScan{0%{background-position:-120% 0%;}100%{background-position:120% 0%;}}
    @keyframes reasonGlow{0%,100%{box-shadow:0 0 0 1px rgba(167,139,250,.10),0 4px 18px -8px rgba(99,102,241,.35),inset 0 0 30px -10px rgba(167,139,250,.08);}50%{box-shadow:0 0 0 1px rgba(167,139,250,.18),0 6px 28px -8px rgba(99,102,241,.55),inset 0 0 40px -10px rgba(167,139,250,.18);}}
    .reasoning-header{display:flex;align-items:center;gap:8px;width:100%;padding:7px 12px;background:transparent;border:0;color:#cbd5e1;font-size:12px;font-weight:500;cursor:pointer;text-align:left;font-family:inherit;}
    .reasoning-header:hover{background:rgba(255,255,255,.02);}
    .reasoning-icon{font-size:13px;}
    .reasoning-active .reasoning-icon{animation:reasonPulse 1.6s ease-in-out infinite;}
    @keyframes reasonPulse{0%,100%{opacity:.55;transform:scale(1);}50%{opacity:1;transform:scale(1.15);}}
    .reasoning-title{flex:1;letter-spacing:.1px;}
    .reasoning-active .reasoning-title{background:linear-gradient(90deg,#cbd5e1 0%,#a78bfa 50%,#cbd5e1 100%);background-size:200% auto;-webkit-background-clip:text;background-clip:text;-webkit-text-fill-color:transparent;animation:reasonTextShine 2.4s linear infinite;}
    @keyframes reasonTextShine{0%{background-position:0% 50%;}100%{background-position:200% 50%;}}
    .reasoning-timer{font-family:var(--mono);font-size:10px;color:#a78bfa;background:rgba(167,139,250,.10);padding:1px 7px;border-radius:10px;border:1px solid rgba(167,139,250,.20);}
    .reasoning-chevron{font-size:10px;opacity:.55;}
    .reasoning-body{padding:2px 14px 12px;font-size:12px;line-height:1.55;color:#94a3b8;font-family:var(--mono);white-space:pre-wrap;border-top:1px solid rgba(167,139,250,.08);max-height:340px;overflow-y:auto;animation:reasonFadeIn .2s ease;}
    @keyframes reasonFadeIn{from{opacity:0;transform:translateY(-2px);}to{opacity:1;transform:none;}}
    :global(:root.light) .msg-reasoning{background:rgba(99,102,241,.04);border-color:rgba(99,102,241,.18);}
    :global(:root.light) .reasoning-header{color:#334155;}
    :global(:root.light) .reasoning-body{color:#475569;}
    :global(.streaming-active){position:relative;}
    :global(.streaming-active) :global(p:last-of-type),:global(.streaming-active) :global(li:last-of-type),:global(.streaming-active) :global(pre:last-of-type){animation:streamReveal .15s ease-out;}
    @keyframes streamReveal{from{opacity:0;transform:translateY(2px);}to{opacity:1;transform:none;}}
    :global(:root.light .msg-lucy){background:#ffffff !important;border:1px solid rgba(0,168,107,.22) !important;border-left:2px solid var(--acc) !important;backdrop-filter:none !important;color:var(--txt) !important;box-shadow:0 1px 6px rgba(0,0,0,.07);}
    :global(:root.light .msg-user){background:rgba(213,228,250,.9) !important;border:1px solid rgba(59,130,246,.18) !important;border-right:2px solid var(--blue) !important;backdrop-filter:none !important;color:var(--txt) !important;}
    :global(:root.light .msg-lucy .mn){color:var(--acc);}
    :global(:root.light .msg-user .mn){color:var(--blue);}
    :global(:root.light .msg-time){color:var(--txt3);}
    :global(:root.light .sys-msg){color:var(--txt3);}
    :global(:root.light .msg-lucy p,:root.light .msg-user p){color:var(--txt) !important;}
    :global(:root.light .msg-lucy strong,:root.light .msg-user strong){color:var(--txt) !important;}
    :global(:root.light .msg-lucy li,:root.light .msg-user li){color:var(--txt) !important;}
    :global(:root.light .msg-lucy code){color:#00875a;background:rgba(0,168,107,.08);}
    :global(:root.light .msg-lucy a){color:var(--blue);}
    :global(:root.light .msg-lucy pre){background:#1a1f2e !important;color:#c8d0dc !important;border:1px solid #2a3448;border-radius:6px;}
    :global(:root.light .msg-lucy th){background:var(--bg3);color:var(--txt);}

    /* U3 — Flip-back button to return to chapter view from linear */
    .chap-flip-back {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        margin: 6px 0 10px;
        padding: 4px 10px;
        font-family: var(--mono, monospace);
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.4px;
        background: rgba(16,185,129,0.08);
        color: var(--acc, #10b981);
        border: 1px solid rgba(16,185,129,0.25);
        border-radius: 4px;
        cursor: pointer;
        transition: background .15s ease, border-color .15s ease;
    }
    .chap-flip-back:hover {
        background: rgba(16,185,129,0.16);
        border-color: rgba(16,185,129,0.45);
    }
</style>
