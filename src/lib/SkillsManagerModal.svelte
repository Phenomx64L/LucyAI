<script>
    import { createEventDispatcher, onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import ConfirmModal from '$lib/ConfirmModal.svelte';
    import { IconBolt as Zap, IconTrash as Trash2, IconPlus as Plus, IconEdit as Edit2, IconPlayerPlay as Play, IconChevronDown as ChevronDown, IconSparkles as Sparkles } from '@tabler/icons-svelte';

    const dispatch = createEventDispatcher();

    // Props
    export let isOpen = false;
    export let isEN = false;

    // Internal state
    let loading = false;
    let loadingSecs = 0;
    let timerInterval = null;
    let error = '';
    let skills = [];
    let selectedCategory = null;
    let formOpen = false;
    let editingSkill = null;
    let executingSkill = null;
    let skillParams = {};

    // ── Favorites (localStorage, no backend needed) ─────────────────────────
    let favorites = new Set();
    function loadFavorites() {
        try { favorites = new Set(JSON.parse(localStorage.getItem('lucy_skill_favorites') || '[]')); }
        catch { favorites = new Set(); }
    }
    function toggleFavorite(id) {
        if (favorites.has(id)) favorites.delete(id); else favorites.add(id);
        favorites = new Set(favorites); // trigger Svelte reactivity
        try { localStorage.setItem('lucy_skill_favorites', JSON.stringify([...favorites])); } catch {}
    }

    let newSkill = {
        id: '',  // generated server-side if empty (new skill), preserved (edit)
        name: '',
        category: 'quick_cmd',
        triggers: JSON.stringify([]),
        script: '',
        description: '',
        parameters: JSON.stringify([]),
        enabled: true,
        tags: JSON.stringify([])
    };

    const labels = {
        'es-MX': {
            title: 'Gestor de Skills',
            name: 'Nombre',
            category: 'Categoría',
            script: 'Script',
            description: 'Descripción',
            parameters: 'Parámetros (JSON)',
            triggers: 'Disparadores (JSON)',
            tags: 'Etiquetas (JSON)',
            add: 'Agregar Skill',
            save: 'Guardar',
            cancel: 'Cancelar',
            delete: 'Eliminar',
            edit: 'Editar',
            execute: 'Ejecutar',
            test: 'Probar',
            noSkills: 'Sin skills definidos',
            error: 'Error',
            loading: 'Cargando...',
            success: 'Guardado',
            enabled: 'Activo',
            usageCount: 'Uso',
            lastExecuted: 'Último uso',
            execution: 'Ejecución de Skill',
            enterParams: 'Ingresar parámetros',
            result: 'Resultado',
            allCategories: 'Todas las categorías',
            quickCmd: 'Comando Rápido',
            runbook: 'Runbook',
            macro: 'Macro',
        },
        'en-US': {
            title: 'Skills Manager',
            name: 'Name',
            category: 'Category',
            script: 'Script',
            description: 'Description',
            parameters: 'Parameters (JSON)',
            triggers: 'Triggers (JSON)',
            tags: 'Tags (JSON)',
            add: 'Add Skill',
            save: 'Save',
            cancel: 'Cancel',
            delete: 'Delete',
            edit: 'Edit',
            execute: 'Execute',
            test: 'Test',
            noSkills: 'No skills defined',
            error: 'Error',
            loading: 'Loading...',
            success: 'Saved',
            enabled: 'Enabled',
            usageCount: 'Usage',
            lastExecuted: 'Last executed',
            execution: 'Skill Execution',
            enterParams: 'Enter parameters',
            result: 'Result',
            allCategories: 'All categories',
            quickCmd: 'Quick Command',
            runbook: 'Runbook',
            macro: 'Macro',
        }
    };

    $: lang = labels[isEN ? 'en-US' : 'es-MX'];

    // Estado del generador IA
    let aiPromptOpen = false;
    let aiIdea = '';
    let aiError = '';

    // Get category label
    function getCategoryLabel(cat) {
        const map = {
            'quick_cmd': lang.quickCmd,
            'runbook': lang.runbook,
            'macro': lang.macro
        };
        return map[cat] || cat;
    }

    // Filter by category, pinned first, then by usage
    $: filteredSkills = (selectedCategory ? skills.filter(s => s.category === selectedCategory) : skills)
        .slice()
        .sort((a, b) => {
            const fa = favorites.has(a.id) ? 0 : 1;
            const fb = favorites.has(b.id) ? 0 : 1;
            return fa - fb || (b.usage_count - a.usage_count);
        });

    // Get unique categories
    $: categories = [...new Set(skills.map(s => s.category))];

    function toast(msg, type = 'info') {
        dispatch('toast', { msg, type });
    }

    async function loadSkills() {
        loading = true;
        error = '';
        try {
            skills = await invoke('list_skills');
        } catch (e) {
            error = String(e);
            toast(lang.error + ': ' + error, 'error');
        }
        loading = false;
    }

    async function saveSkill() {
        if (!newSkill.name.trim()) {
            error = 'Name is required';
            return;
        }
        if (!newSkill.script.trim()) {
            error = 'Script is required';
            return;
        }

        loading = true;
        error = '';
        try {
            // Validate JSON fields
            try {
                JSON.parse(newSkill.parameters);
                JSON.parse(newSkill.triggers);
                JSON.parse(newSkill.tags);
            } catch (e) {
                throw new Error('Invalid JSON in parameters/triggers/tags: ' + e.message);
            }

            const skillData = {
                id: editingSkill ? editingSkill.id : '',
                ...newSkill,
                enabled: newSkill.enabled ? 1 : 0,
                created_at: editingSkill ? editingSkill.created_at : new Date().toISOString(),
                updated_at: new Date().toISOString(),
                usage_count: editingSkill ? editingSkill.usage_count : 0,
                last_executed: editingSkill ? editingSkill.last_executed : null
            };

            const savedId = await invoke('save_skill', { skill: skillData });
            // Semantic index: fire-and-forget; silently skip if Ollama is offline.
            const embedText = `${skillData.name}\n${skillData.description || ''}\n${skillData.triggers || ''}`;
            invoke('upsert_embedding', { entityType: 'skill', entityId: savedId || skillData.id, text: embedText })
                .catch(err => console.debug('[embed] skill skipped:', err));
            toast(lang.success, 'success');
            formOpen = false;
            editingSkill = null;
            resetForm();
            await loadSkills();
        } catch (e) {
            error = String(e);
            toast(lang.error + ': ' + error, 'error');
        }
        loading = false;
    }

    function openAIPrompt() {
        aiIdea = '';
        aiError = '';
        aiPromptOpen = true;
        loading = false; // reset por si quedó stuck
    }

    async function generateSkillWithAI() {
        const idea = aiIdea.trim();
        if (!idea) { aiError = 'Describe quǸ skill quieres generar'; return; }
        aiError = '';
        loading = true; error = '';
        loadingSecs = 0;
        
        timerInterval = setInterval(() => {
            loadingSecs++;
        }, 1000);

        try {
            // Re-enforcamos el requerimiento al final
            const ideaStrict = idea + " (Responde SOLO con JSON, nada más)";
            
            // 20 second timeout protect en Frontend
            const timeoutPromise = new Promise((_, reject) => setTimeout(() => reject(new Error('Tiempo de espera agotado (20s). Los servidores de Gemini están sobrecargados o hay latencia de red.')), 20000));
            const raw = await Promise.race([
                invoke('generate_skill_template', {
                    idea: ideaStrict,
                    model: 'gemini-2.5-flash'
                }),
                timeoutPromise
            ]);
            
            clearInterval(timerInterval);
            
            // Extraer JSON del texto de respuesta
            const txt = String(raw);
            const m = txt.match(/\{[\s\S]*\}/);
            if (!m) throw new Error('La IA no devolvió JSON válido. Respuesta:\n' + txt.slice(0, 100));
            
            const parsed = JSON.parse(m[0]);
            
            newSkill = {
                name: parsed.name || 'Skill generada',
                category: ['monitoring','admin','network','security','diagnostics'].includes(parsed.category) ? parsed.category : 'admin',
                description: parsed.description || '',
                script: parsed.script || '',
                parameters: parsed.parameters ? JSON.stringify(parsed.parameters, null, 2) : '[]',
                triggers: parsed.triggers ? JSON.stringify(parsed.triggers, null, 2) : '[]',
                tags: parsed.tags ? JSON.stringify(parsed.tags, null, 2) : '[]',
                enabled: true
            };
            aiPromptOpen = false;
            formOpen = true; // abrir form con la data rellenada
            toast('Skill generada por IA — revisa y guarda', 'success');
        } catch (e) {
            clearInterval(timerInterval);
            error = String(e);
            aiError = String(e).slice(0, 400); // mostrar error directo en la caja
            toast('Error de IA: ' + String(e).slice(0, 50), 'error');
        }
        loading = false;
        clearInterval(timerInterval);
    }

    let pendingDelete = null;   // {id, name} | null
    function deleteSkill(id) {
        const sk = skills.find(s => s.id === id);
        pendingDelete = { id, name: sk?.name || '' };
    }
    async function confirmDeleteSkill() {
        if (!pendingDelete) return;
        const { id } = pendingDelete;
        pendingDelete = null;
        loading = true;
        try {
            await invoke('delete_skill', { skillId: id });
            invoke('delete_embedding', { entityType: 'skill', entityId: id })
                .catch(err => console.debug('[embed] delete skipped:', err));
            toast(lang.success, 'success');
            await loadSkills();
        } catch (e) {
            error = String(e);
            toast(lang.error + ': ' + error, 'error');
        }
        loading = false;
    }

    async function incrementUsage(id) {
        try {
            await invoke('increment_skill_usage', { skillId: id });
            await loadSkills();
        } catch (e) {
            // Silent fail for usage increment
        }
    }

    function editSkill(skill) {
        newSkill = {
            id: skill.id,  // CRITICAL: preserve ID for UPDATE instead of INSERT
            name: skill.name,
            category: skill.category,
            triggers: skill.triggers,
            script: skill.script,
            description: skill.description,
            parameters: skill.parameters,
            enabled: skill.enabled,
            tags: skill.tags
        };
        editingSkill = skill;
        formOpen = true;
        skillParams = {};
    }

    function prepareExecution(skill) {
        executingSkill = skill;
        skillParams = {};
        // Parse parameters if they exist
        try {
            const params = JSON.parse(skill.parameters || '[]');
            params.forEach(p => {
                skillParams[p.name] = '';
            });
        } catch (e) {
            // No parameters
        }
    }

    async function executeSkill() {
        if (!executingSkill) return;

        loading = true;
        error = '';
        try {
            // Substitute parameters in script
            let script = executingSkill.script;
            Object.entries(skillParams).forEach(([key, value]) => {
                script = script.replace(new RegExp(`{{${key}}}`, 'g'), value);
            });

            // Execute the script (assuming it's a command)
            // For now, we'll just increment usage and show success
            await incrementUsage(executingSkill.id);

            toast(lang.success + ': ' + executingSkill.name, 'success');
            executingSkill = null;
            skillParams = {};
            await loadSkills();
        } catch (e) {
            error = String(e);
            toast(lang.error + ': ' + error, 'error');
        }
        loading = false;
    }

    function resetForm() {
        newSkill = {
            name: '',
            category: 'quick_cmd',
            triggers: JSON.stringify([]),
            script: '',
            description: '',
            parameters: JSON.stringify([]),
            enabled: true,
            tags: JSON.stringify([])
        };
        editingSkill = null;
        error = '';
    }

    function openAddForm() {
        resetForm();
        formOpen = true;
    }

    function closeModal() {
        isOpen = false;
        resetForm();
        executingSkill = null;
        skillParams = {};
        dispatch('close');
    }

    onMount(() => {
        loadFavorites();
        loading = false; // reset defensivo (HMR puede dejarlo stuck)
        if (isOpen && skills.length === 0) loadSkills();
    });

    $: if (isOpen && skills.length === 0) loadSkills();
</script>

{#if isOpen}
    <div class="modal-overlay" on:click={closeModal}>
        <div class="modal-content" role="dialog" on:click|stopPropagation>
            <!-- Header -->
            <div class="modal-header">
                <h2><Zap size={20} /> {lang.title}</h2>
                <button class="close-btn" on:click={closeModal}>✕</button>
            </div>

            <!-- Body -->
            <div class="modal-body">
                {#if error}
                    <div class="error-box">{error}</div>
                {/if}

                <!-- Execution Modal (overlay) -->
                {#if executingSkill}
                    <div class="execution-panel">
                        <div class="exec-header">
                            <h3>{lang.execution}: {executingSkill.name}</h3>
                            <button class="close-btn" on:click={() => executingSkill = null}>✕</button>
                        </div>

                        <!-- Parameter inputs -->
                        {#if Object.keys(skillParams).length > 0}
                            <div class="params-section">
                                <h4>{lang.enterParams}</h4>
                                {#each Object.keys(skillParams) as param}
                                    <div class="param-input">
                                        <label>{param}</label>
                                        <input
                                            type="text"
                                            bind:value={skillParams[param]}
                                            placeholder="Enter value"
                                        />
                                    </div>
                                {/each}
                            </div>
                        {/if}

                        <!-- Script preview -->
                        <div class="script-preview">
                            <h4>Script Preview</h4>
                            <pre>{executingSkill.script}</pre>
                        </div>

                        <!-- Actions -->
                        <div class="exec-actions">
                            <button class="btn-execute" on:click={executeSkill} disabled={loading}>
                                {loading ? lang.loading : lang.execute}
                            </button>
                            <button class="btn-cancel" on:click={() => executingSkill = null}>
                                {lang.cancel}
                            </button>
                        </div>
                    </div>
                {:else if formOpen}
                    <!-- Form Panel -->
                    <div class="form-panel">
                        <h3>{editingSkill ? lang.edit : lang.add} Skill</h3>

                        <div class="form-group">
                            <label>{lang.name}</label>
                            <input
                                type="text"
                                bind:value={newSkill.name}
                                placeholder="e.g. restart-service"
                            />
                        </div>

                        <div class="form-row">
                            <div class="form-group">
                                <label>{lang.category}</label>
                                <select bind:value={newSkill.category}>
                                    <option value="quick_cmd">{lang.quickCmd}</option>
                                    <option value="runbook">{lang.runbook}</option>
                                    <option value="macro">{lang.macro}</option>
                                </select>
                            </div>
                            <div class="form-group checkbox">
                                <label>
                                    <input type="checkbox" bind:checked={newSkill.enabled} />
                                    {lang.enabled}
                                </label>
                            </div>
                        </div>

                        <div class="form-group">
                            <label>{lang.description}</label>
                            <textarea bind:value={newSkill.description} placeholder="Optional description"></textarea>
                        </div>

                        <div class="form-group">
                            <label>{lang.script}</label>
                            <textarea bind:value={newSkill.script} placeholder={'Script content (supports {variable} substitution)'}></textarea>
                        </div>

                        <div class="form-row">
                            <div class="form-group">
                                <label>{lang.parameters}</label>
                                <textarea
                                    bind:value={newSkill.parameters}
                                    placeholder="JSON array of parameters"
                                    rows="3"
                                ></textarea>
                            </div>
                            <div class="form-group">
                                <label>{lang.triggers}</label>
                                <textarea
                                    bind:value={newSkill.triggers}
                                    placeholder="JSON array of trigger phrases"
                                    rows="3"
                                ></textarea>
                            </div>
                        </div>

                        <div class="form-group">
                            <label>{lang.tags}</label>
                            <textarea
                                bind:value={newSkill.tags}
                                placeholder="JSON array of tags"
                                rows="2"
                            ></textarea>
                        </div>

                        <!-- Form Actions -->
                        <div class="form-actions">
                            <button class="btn-save" on:click={saveSkill} disabled={loading}>
                                {loading ? lang.loading : lang.save}
                            </button>
                            <button class="btn-cancel" on:click={() => { formOpen = false; resetForm(); }}>
                                {lang.cancel}
                            </button>
                        </div>
                    </div>
                {:else}
                    <!-- Skills List -->
                    <div class="skills-section">
                        <!-- Category Filter -->
                        <div class="filter-bar">
                            <button
                                class:active={selectedCategory === null}
                                on:click={() => selectedCategory = null}
                            >
                                {lang.allCategories}
                            </button>
                            {#each categories as cat}
                                <button
                                    class:active={selectedCategory === cat}
                                    on:click={() => selectedCategory = cat}
                                >
                                    {getCategoryLabel(cat)}
                                </button>
                            {/each}
                        </div>

                        {#if filteredSkills.length === 0}
                            <div class="no-skills">{lang.noSkills}</div>
                        {:else}
                            <div class="skills-grid">
                                {#each filteredSkills as skill (skill.id)}
                                    <div class="skill-card" class:skill-pinned={favorites.has(skill.id)}>
                                        <div class="skill-header">
                                            <div>
                                                <h4>{skill.name}</h4>
                                                <span class="category-badge">{getCategoryLabel(skill.category)}</span>
                                            </div>
                                            <div class="skill-status">
                                                <button class="btn-fav" class:fav-active={favorites.has(skill.id)}
                                                    on:click|stopPropagation={() => toggleFavorite(skill.id)}
                                                    title={favorites.has(skill.id) ? (isEN ? 'Unpin' : 'Quitar favorito') : (isEN ? 'Pin to top' : 'Marcar favorito')}>
                                                    {favorites.has(skill.id) ? '★' : '☆'}
                                                </button>
                                                {#if skill.enabled}
                                                    <span class="enabled">✓</span>
                                                {/if}
                                            </div>
                                        </div>

                                        {#if skill.description}
                                            <p class="description">{skill.description}</p>
                                        {/if}

                                        <div class="skill-meta">
                                            <span>{lang.usageCount}: {skill.usage_count}</span>
                                            {#if skill.last_executed}
                                                <span>{lang.lastExecuted}: {new Date(skill.last_executed).toLocaleDateString(isEN ? 'en-US' : 'es-MX')}</span>
                                            {/if}
                                        </div>

                                        <div class="skill-actions">
                                            <button class="btn-execute" on:click={() => prepareExecution(skill)} title={lang.execute}>
                                                <Play size={14} />
                                            </button>
                                            <button class="btn-edit" on:click={() => editSkill(skill)} title={lang.edit}>
                                                <Edit2 size={14} />
                                            </button>
                                            <button class="btn-delete" on:click={() => deleteSkill(skill.id)} title={lang.delete}>
                                                <Trash2 size={14} />
                                            </button>
                                        </div>
                                    </div>
                                {/each}
                            </div>
                        {/if}

                        <button class="btn-add" on:click={openAddForm}>
                            <Plus size={16} /> {lang.add}
                        </button>
                        <button class="btn-add" style="margin-left:8px;background:linear-gradient(135deg,#7c3aed,#a855f7);display:flex;align-items:center;gap:5px;" on:click={openAIPrompt} disabled={loading && aiPromptOpen}>
                            <Sparkles size={13} strokeWidth={2}/> Generar con IA
                        </button>

                        {#if aiPromptOpen}
                        <div style="margin-top:14px;padding:14px;background:rgba(124,58,237,.08);border:1px solid rgba(124,58,237,.3);border-radius:8px;">
                            <div style="font-size:13px;font-weight:600;margin-bottom:8px;color:#c4b5fd;display:flex;align-items:center;gap:6px;"><Sparkles size={13} strokeWidth={2}/> Generar skill con IA</div>
                            <div style="font-size:11px;color:#94a3b8;margin-bottom:10px;">Describe qué quieres que haga la skill. Lucy la generará automáticamente con script PowerShell, parámetros, triggers y tags.</div>
                            <input type="text" bind:value={aiIdea}
                                placeholder='Ej: "reporte de uso de disco", "reiniciar IIS", "listar procesos con más CPU"'
                                style="width:100%;padding:8px 10px;background:rgba(0,0,0,.3);border:1px solid rgba(124,58,237,.4);border-radius:5px;color:#fff;font-size:12px;font-family:inherit;"
                                on:keydown={(e) => { if (e.key === 'Enter' && !loading) generateSkillWithAI(); if (e.key === 'Escape') aiPromptOpen = false; }} />
                            {#if aiError}
                                <div style="color:#f87171;font-size:11px;margin-top:6px;font-family:monospace;white-space:pre-wrap;">{aiError}</div>
                            {/if}
                            <div style="display:flex;gap:8px;margin-top:10px;">
                                <button class="btn-add" style="background:linear-gradient(135deg,#7c3aed,#a855f7);display:flex;align-items:center;gap:5px;" on:click={generateSkillWithAI} disabled={loading}>
                                    {#if loading}↻ Generando ({loadingSecs}s)…{:else}<Sparkles size={12} strokeWidth={2}/> Generar{/if}
                                </button>
                                <button class="btn-cancel" on:click={() => { aiPromptOpen = false; loading = false; }} disabled={loading}>
                                    Cancelar
                                </button>
                            </div>
                        </div>
                        {/if}
                    </div>
                {/if}
            </div>
        </div>
    </div>
{/if}

<ConfirmModal
    open={pendingDelete !== null}
    variant="danger"
    title={lang.delete}
    message={isEN
        ? 'Delete this skill? This action cannot be undone.'
        : '¿Eliminar esta skill? Esta acción no se puede deshacer.'}
    detail={pendingDelete?.name || ''}
    confirmLabel={lang.delete}
    cancelLabel={isEN ? 'Cancel' : 'Cancelar'}
    on:confirm={confirmDeleteSkill}
    on:cancel={() => pendingDelete = null}
/>

<style>
    .modal-overlay {
        position: fixed;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        background: rgba(0, 0, 0, 0.7);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: var(--z-modal, 2000);
    }

    .modal-content {
        background: #1a1a2e;
        border: 1px solid #3a3a5c;
        border-radius: 8px;
        color: #e0e0e0;
        width: 90%;
        max-width: 1200px;
        max-height: 90vh;
        display: flex;
        flex-direction: column;
        box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
    }

    .modal-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 1.5rem;
        border-bottom: 1px solid #3a3a5c;
    }

    .modal-header h2 {
        margin: 0;
        font-size: 1.3rem;
        display: flex;
        align-items: center;
        gap: 0.5rem;
        color: #fff;
    }

    .close-btn {
        background: none;
        border: none;
        color: #888;
        font-size: 1.5rem;
        cursor: pointer;
        padding: 0;
        width: 30px;
        height: 30px;
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .close-btn:hover {
        color: #fff;
    }

    .modal-body {
        flex: 1;
        overflow-y: auto;
        padding: 1.5rem;
    }

    .error-box {
        padding: 1rem;
        background: #3e1b1b;
        border: 1px solid #8b3333;
        border-radius: 4px;
        color: #ff9999;
        margin-bottom: 1rem;
    }

    /* Form Styles */
    .form-panel {
        background: #0f3460;
        padding: 1.5rem;
        border: 1px solid #3a3a5c;
        border-radius: 6px;
    }

    .form-panel h3 {
        margin: 0 0 1.5rem 0;
        font-size: 1.1rem;
        color: #fff;
    }

    .form-group {
        margin-bottom: 1.5rem;
    }

    .form-group label {
        display: block;
        margin-bottom: 0.5rem;
        font-size: 0.85rem;
        color: #888;
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }

    input[type="text"],
    textarea,
    select {
        width: 100%;
        padding: 0.75rem;
        background: #1a1a2e;
        border: 1px solid #3a3a5c;
        border-radius: 4px;
        color: #e0e0e0;
        font-family: inherit;
        font-size: 0.9rem;
    }

    textarea {
        resize: vertical;
        min-height: 60px;
        font-family: 'Courier New', monospace;
    }

    input[type="text"]:focus,
    textarea:focus,
    select:focus {
        outline: none;
        border-color: #4a9eff;
        background: #0f3460;
    }

    .form-row {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 1rem;
    }

    .form-group.checkbox {
        display: flex;
        align-items: center;
        margin-bottom: 0;
    }

    .form-group.checkbox label {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        margin: 0;
        text-transform: none;
        color: #e0e0e0;
    }

    input[type="checkbox"] {
        width: auto;
    }

    .form-actions {
        display: flex;
        gap: 1rem;
        justify-content: flex-end;
        margin-top: 2rem;
        padding-top: 1rem;
        border-top: 1px solid #3a3a5c;
    }

    .btn-save,
    .btn-cancel {
        padding: 0.75rem 1.5rem;
        border: none;
        border-radius: 4px;
        cursor: pointer;
        font-weight: 500;
    }

    .btn-save {
        background: #4a9eff;
        color: #fff;
    }

    .btn-save:hover:not(:disabled) {
        background: #3a8eef;
    }

    .btn-save:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .btn-cancel {
        background: #3a3a5c;
        color: #e0e0e0;
    }

    .btn-cancel:hover {
        background: #4a4a7c;
    }

    /* Skills List */
    .skills-section {
        display: flex;
        flex-direction: column;
        gap: 1.5rem;
    }

    .filter-bar {
        display: flex;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .filter-bar button {
        padding: 0.5rem 1rem;
        background: #0f3460;
        border: 1px solid #3a3a5c;
        border-radius: 4px;
        color: #888;
        cursor: pointer;
        font-size: 0.9rem;
        transition: all 0.2s;
    }

    .filter-bar button:hover {
        border-color: #4a9eff;
        color: #4a9eff;
    }

    .filter-bar button.active {
        background: #4a9eff;
        color: #fff;
        border-color: #4a9eff;
    }

    .no-skills {
        text-align: center;
        padding: 3rem 1rem;
        color: #666;
    }

    .skills-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
        gap: 1rem;
    }

    .skill-card {
        background: #0f3460;
        border: 1px solid #3a3a5c;
        border-radius: 6px;
        padding: 1rem;
        display: flex;
        flex-direction: column;
        gap: 1rem;
        transition: all 0.2s;
    }

    .skill-card:hover {
        border-color: #4a9eff;
        background: #1a3a52;
    }

    .skill-header {
        display: flex;
        justify-content: space-between;
        align-items: flex-start;
        gap: 1rem;
    }

    .skill-header h4 {
        margin: 0;
        color: #fff;
        font-size: 1rem;
    }

    .category-badge {
        display: inline-block;
        padding: 0.25rem 0.75rem;
        background: #4a9eff;
        color: #fff;
        border-radius: 3px;
        font-size: 0.75rem;
        margin-top: 0.5rem;
    }

    .skill-status {
        display: flex;
        align-items: center;
        gap: 6px;
        color: #66ff66;
        font-weight: bold;
    }

    /* ── Favorites ─────────────────────────────────────────────────────── */
    .btn-fav {
        background: none;
        border: none;
        cursor: pointer;
        font-size: 15px;
        color: #475569;
        padding: 0 2px;
        line-height: 1;
        transition: color .15s, transform .15s;
    }
    .btn-fav:hover { color: #fbbf24; transform: scale(1.15); }
    .btn-fav.fav-active { color: #fbbf24; }
    .skill-pinned {
        border-color: rgba(251,191,36,0.35) !important;
        box-shadow: 0 0 0 1px rgba(251,191,36,0.18) !important;
    }

    .description {
        margin: 0;
        font-size: 0.9rem;
        color: #aaa;
        line-height: 1.4;
    }

    .skill-meta {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        font-size: 0.8rem;
        color: #666;
    }

    .skill-actions {
        display: flex;
        gap: 0.5rem;
        justify-content: flex-end;
    }

    .btn-execute,
    .btn-edit,
    .btn-delete {
        background: none;
        border: none;
        color: #888;
        cursor: pointer;
        padding: 0.5rem;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 4px;
        transition: all 0.2s;
    }

    .btn-execute:hover {
        background: #1a4a7a;
        color: #4a9eff;
    }

    .btn-edit:hover {
        background: #1a4a7a;
        color: #ffb347;
    }

    .btn-delete:hover {
        background: #4a1a1a;
        color: #ff6666;
    }

    .btn-add {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.75rem 1.5rem;
        background: #4a9eff;
        border: none;
        border-radius: 4px;
        color: #fff;
        cursor: pointer;
        font-weight: 500;
        align-self: flex-start;
        margin-top: 1rem;
    }

    .btn-add:hover {
        background: #3a8eef;
    }

    /* Execution Panel */
    .execution-panel {
        background: #0f3460;
        border: 2px solid #4a9eff;
        padding: 1.5rem;
        border-radius: 6px;
        position: relative;
    }

    .exec-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        margin-bottom: 1.5rem;
        padding-bottom: 1rem;
        border-bottom: 1px solid #3a3a5c;
    }

    .exec-header h3 {
        margin: 0;
        color: #4a9eff;
    }

    .params-section {
        margin-bottom: 1.5rem;
        padding-bottom: 1rem;
        border-bottom: 1px solid #3a3a5c;
    }

    .params-section h4 {
        margin: 0 0 1rem 0;
        font-size: 0.95rem;
        color: #888;
        text-transform: uppercase;
    }

    .param-input {
        margin-bottom: 1rem;
    }

    .param-input label {
        display: block;
        margin-bottom: 0.5rem;
        font-size: 0.9rem;
        color: #e0e0e0;
        font-weight: 500;
    }

    .param-input input {
        width: 100%;
        padding: 0.75rem;
        background: #1a1a2e;
        border: 1px solid #3a3a5c;
        border-radius: 4px;
        color: #e0e0e0;
    }

    .param-input input:focus {
        outline: none;
        border-color: #4a9eff;
    }

    .script-preview {
        margin-bottom: 1.5rem;
    }

    .script-preview h4 {
        margin: 0 0 0.75rem 0;
        font-size: 0.95rem;
        color: #888;
        text-transform: uppercase;
    }

    .script-preview pre {
        margin: 0;
        padding: 1rem;
        background: #1a1a2e;
        border: 1px solid #3a3a5c;
        border-radius: 4px;
        color: #4a9eff;
        font-size: 0.85rem;
        overflow-x: auto;
        max-height: 200px;
        font-family: 'Courier New', monospace;
    }

    .exec-actions {
        display: flex;
        gap: 1rem;
        justify-content: flex-end;
    }

    .btn-execute {
        padding: 0.75rem 1.5rem;
        background: #4a9eff;
        border: none;
        border-radius: 4px;
        color: #fff;
        cursor: pointer;
        font-weight: 500;
    }

    .btn-execute:hover:not(:disabled) {
        background: #3a8eef;
    }

    .btn-execute:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    @media (max-width: 768px) {
        .modal-content {
            width: 95%;
            max-height: 95vh;
        }

        .form-row {
            grid-template-columns: 1fr;
        }

        .skills-grid {
            grid-template-columns: 1fr;
        }

        .skill-card {
            padding: 0.75rem;
        }
    }
</style>
