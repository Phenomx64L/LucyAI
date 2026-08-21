<!--
  AccentSwatches.svelte — v1.7.98 (Option D5)

  Six small swatches that let the operator pick a primary accent hue
  WITHOUT changing the active warp theme (gradient backdrop). The two
  axes are orthogonal:

    • setWarpTheme()  → background gradient ('default', 'ocean', …)
    • applyAccent()   → primary action color ('emerald', 'violet', …)

  Mounted next to the existing theme-picker-grid in +page.svelte's
  settings panel. Visual language matches the theme dots so the panel
  stays consistent.
-->
<script lang="ts">
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import {
        ACCENT_PRESETS,
        applyAccent,
        type AccentId,
    } from '$lib/accent-store';

    export let active: AccentId = 'emerald';

    function pick(id: AccentId) {
        applyAccent(id);
        active = id;
    }
</script>

<div class="accent-row" title={$trad('Color de acento')}>
    <span class="accent-row-label">{$trad('Acento')}</span>
    <div class="accent-swatches">
        {#each ACCENT_PRESETS as a (a.id)}
            <button
                type="button"
                class="accent-swatch"
                class:active={active === a.id}
                style="--swatch-color: {a.accent}; --swatch-glow: {a.border};"
                aria-label={a.label}
                title={a.label}
                on:click={() => pick(a.id)}
            ></button>
        {/each}
    </div>
</div>

<style>
    .accent-row {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 6px 0 2px;
    }
    .accent-row-label {
        font-size: 11px;
        color: var(--text-muted);
        letter-spacing: 0.4px;
        text-transform: uppercase;
        font-weight: 600;
        flex-shrink: 0;
    }
    .accent-swatches {
        display: flex;
        gap: 6px;
        flex: 1;
    }
    .accent-swatch {
        width: 18px;
        height: 18px;
        border-radius: 50%;
        border: 1.5px solid rgba(255, 255, 255, 0.10);
        background: var(--swatch-color);
        cursor: pointer;
        padding: 0;
        transition: transform var(--motion-fast) var(--ease-out),
                    border-color var(--motion-fast) var(--ease-out),
                    box-shadow var(--motion-fast) var(--ease-out);
    }
    .accent-swatch:hover {
        transform: scale(1.18);
        border-color: rgba(255, 255, 255, 0.30);
    }
    .accent-swatch.active {
        border-color: var(--swatch-color);
        box-shadow: 0 0 0 2px var(--swatch-glow),
                    0 0 10px 0 var(--swatch-glow);
    }
</style>
