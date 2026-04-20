<script>
    import { createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { IconSettings as Settings, IconTrash as Trash2, IconPlus as Plus, IconCheck as Check, IconX as X } from '@tabler/icons-svelte';

    const dispatch = createEventDispatcher();

    // Props
    export let isOpen = false;
    export let isEN = false;

    // Internal state
    let loading = false;
    let error = '';
    let rules = [];
    let testCmd = '';
    let testResult = null;
    let editingRule = null;
    let formOpen = false;
    let newRule = {
        pattern: '',
        action: 'allow',
        description: '',
        priority: 0,
        applies_to: 'command',
        enabled: true
    };

    const labels = {
        'es-MX': {
            title: 'Reglas de Permisos',
            pattern: 'Patrón (Regex)',
            action: 'Acción',
            description: 'Descripción',
            priority: 'Prioridad',
            appliesTo: 'Aplica a',
            enabled: 'Activo',
            add: 'Agregar Regla',
            save: 'Guardar',
            cancel: 'Cancelar',
            delete: 'Eliminar',
            test: 'Probar',
            testCmd: 'Comando a Probar',
            noRules: 'Sin reglas definidas',
            error: 'Error',
            loading: 'Cargando...',
            success: 'Guardado',
            allow: 'Permitir',
            block: 'Bloquear',
            ask: 'Preguntar',
            command: 'Comando',
            filePath: 'Ruta de Archivo',
            registry: 'Registro',
            matched: 'Regla coincidida',
            notMatched: 'Ninguna regla coincidió',
        },
        'en-US': {
            title: 'Permission Rules',
            pattern: 'Pattern (Regex)',
            action: 'Action',
            description: 'Description',
            priority: 'Priority',
            appliesTo: 'Applies To',
            enabled: 'Enabled',
            add: 'Add Rule',
            save: 'Save',
            cancel: 'Cancel',
            delete: 'Delete',
            test: 'Test',
            testCmd: 'Command to Test',
            noRules: 'No rules defined',
            error: 'Error',
            loading: 'Loading...',
            success: 'Saved',
            allow: 'Allow',
            block: 'Block',
            ask: 'Ask',
            command: 'Command',
            filePath: 'File Path',
            registry: 'Registry',
            matched: 'Rule matched',
            notMatched: 'No rules matched',
        }
    };

    $: lang = labels[isEN ? 'en-US' : 'es-MX'];

    function toast(msg, type = 'info') {
        dispatch('toast', { msg, type });
    }

    async function loadRules() {
        loading = true;
        error = '';
        try {
            rules = await invoke('list_permission_rules');
        } catch (e) {
            error = String(e);
            toast(lang.error + ': ' + error, 'error');
        }
        loading = false;
    }

    async function saveRule() {
        if (!newRule.pattern.trim()) {
            error = 'Pattern is required';
            return;
        }
        loading = true;
        error = '';
        try {
            await invoke('save_permission_rule', { rule: newRule });
            toast(lang.success, 'success');
            formOpen = false;
            editingRule = null;
            resetForm();
            await loadRules();
        } catch (e) {
            error = String(e);
            toast(lang.error + ': ' + error, 'error');
        }
        loading = false;
    }

    async function deleteRule(id) {
        if (!confirm(lang.delete + '?')) return;
        loading = true;
        try {
            await invoke('delete_permission_rule', { ruleId: id });
            toast(lang.success, 'success');
            await loadRules();
        } catch (e) {
            error = String(e);
            toast(lang.error + ': ' + error, 'error');
        }
        loading = false;
    }

    async function testRule() {
        if (!testCmd.trim()) {
            error = 'Enter a command to test';
            return;
        }
        try {
            testResult = await invoke('check_permission', {
                cmd: testCmd,
                ruleType: newRule.applies_to
            });
        } catch (e) {
            error = String(e);
        }
    }

    function editRule(rule) {
        newRule = { ...rule };
        editingRule = rule.id;
        formOpen = true;
    }

    function resetForm() {
        newRule = {
            pattern: '',
            action: 'allow',
            description: '',
            priority: 0,
            applies_to: 'command',
            enabled: true
        };
        editingRule = null;
        error = '';
        testResult = null;
        testCmd = '';
    }

    function openAddForm() {
        resetForm();
        formOpen = true;
    }

    function closeModal() {
        isOpen = false;
        resetForm();
        dispatch('close');
    }

    $: if (isOpen && rules.length === 0) loadRules();
</script>

{#if isOpen}
    <div class="modal-overlay" on:click={closeModal}>
        <div class="modal-content" on:click|stopPropagation>
            <!-- Header -->
            <div class="modal-header">
                <h2><Settings size={20} /> {lang.title}</h2>
                <button class="close-btn" on:click={closeModal}>✕</button>
            </div>

            <!-- Body -->
            <div class="modal-body">
                {#if error}
                    <div class="error-box">{error}</div>
                {/if}

                <!-- Rules List / Form Toggle -->
                <div class="rules-section">
                    {#if formOpen}
                        <!-- Form -->
                        <div class="form-panel">
                            <h3>{editingRule ? 'Edit' : 'New'} Rule</h3>

                            <div class="form-group">
                                <label>{lang.pattern}</label>
                                <input
                                    type="text"
                                    bind:value={newRule.pattern}
                                    placeholder="e.g. netsh.*interface|C:\\Users\\.*\.exe"
                                />
                            </div>

                            <div class="form-row">
                                <div class="form-group">
                                    <label>{lang.action}</label>
                                    <select bind:value={newRule.action}>
                                        <option value="allow">{lang.allow}</option>
                                        <option value="block">{lang.block}</option>
                                        <option value="ask">{lang.ask}</option>
                                    </select>
                                </div>
                                <div class="form-group">
                                    <label>{lang.appliesTo}</label>
                                    <select bind:value={newRule.applies_to}>
                                        <option value="command">{lang.command}</option>
                                        <option value="file_path">{lang.filePath}</option>
                                        <option value="registry">{lang.registry}</option>
                                    </select>
                                </div>
                            </div>

                            <div class="form-group">
                                <label>{lang.description}</label>
                                <textarea bind:value={newRule.description} placeholder="Optional description"></textarea>
                            </div>

                            <div class="form-row">
                                <div class="form-group">
                                    <label>{lang.priority}</label>
                                    <input type="number" bind:value={newRule.priority} min="0" />
                                </div>
                                <div class="form-group checkbox">
                                    <label>
                                        <input type="checkbox" bind:checked={newRule.enabled} />
                                        {lang.enabled}
                                    </label>
                                </div>
                            </div>

                            <!-- Test Section -->
                            <div class="test-section">
                                <h4>Test Regex</h4>
                                <div class="form-group">
                                    <input
                                        type="text"
                                        bind:value={testCmd}
                                        placeholder={lang.testCmd}
                                    />
                                    <button on:click={testRule} class="test-btn">
                                        {lang.test}
                                    </button>
                                </div>
                                {#if testResult}
                                    <div class="test-result" class:matched={testResult.action !== 'allow'}>
                                        <strong>{testResult.action === 'allow' ? lang.notMatched : lang.matched}</strong>
                                        {#if testResult.rule_id}
                                            <div>{testResult.reason}</div>
                                        {/if}
                                    </div>
                                {/if}
                            </div>

                            <!-- Form Actions -->
                            <div class="form-actions">
                                <button class="btn-save" on:click={saveRule} disabled={loading}>
                                    {loading ? lang.loading : lang.save}
                                </button>
                                <button class="btn-cancel" on:click={() => { formOpen = false; resetForm(); }}>
                                    {lang.cancel}
                                </button>
                            </div>
                        </div>
                    {:else}
                        <!-- Rules List -->
                        {#if rules.length === 0}
                            <div class="no-rules">{lang.noRules}</div>
                        {:else}
                            <table class="rules-table">
                                <thead>
                                    <tr>
                                        <th>{lang.priority}</th>
                                        <th>{lang.pattern}</th>
                                        <th>{lang.action}</th>
                                        <th>{lang.appliesTo}</th>
                                        <th>{lang.enabled}</th>
                                        <th>Actions</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {#each rules as rule (rule.id)}
                                        <tr>
                                            <td class="priority">{rule.priority}</td>
                                            <td class="pattern" title={rule.pattern}>{rule.pattern.substring(0, 30)}{rule.pattern.length > 30 ? '...' : ''}</td>
                                            <td class="action" class:allow={rule.action === 'allow'} class:block={rule.action === 'block'} class:ask={rule.action === 'ask'}>
                                                {rule.action.toUpperCase()}
                                            </td>
                                            <td>{rule.applies_to}</td>
                                            <td class="enabled">
                                                {#if rule.enabled}
                                                    <Check size={16} />
                                                {:else}
                                                    <X size={16} />
                                                {/if}
                                            </td>
                                            <td class="actions">
                                                <button class="edit-btn" on:click={() => editRule(rule)} title="Edit">✏️</button>
                                                <button class="delete-btn" on:click={() => deleteRule(rule.id)} title="Delete">
                                                    <Trash2 size={14} />
                                                </button>
                                            </td>
                                        </tr>
                                    {/each}
                                </tbody>
                            </table>
                        {/if}
                        <button class="btn-add" on:click={openAddForm}>
                            <Plus size={16} /> {lang.add}
                        </button>
                    {/if}
                </div>
            </div>
        </div>
    </div>
{/if}

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
        max-width: 1000px;
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

    .rules-section {
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    .no-rules {
        text-align: center;
        padding: 3rem 1rem;
        color: #666;
    }

    /* Form Styles */
    .form-panel {
        background: #0f3460;
        padding: 1.5rem;
        border: 1px solid #3a3a5c;
        border-radius: 6px;
    }

    .form-panel h3 {
        margin: 0 0 1rem 0;
        font-size: 1.1rem;
        color: #fff;
    }

    .form-group {
        margin-bottom: 1rem;
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
    input[type="number"],
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
        min-height: 60px;
        resize: vertical;
    }

    input[type="text"]:focus,
    input[type="number"]:focus,
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

    /* Test Section */
    .test-section {
        background: #16213e;
        padding: 1rem;
        border-radius: 4px;
        margin: 1rem 0;
    }

    .test-section h4 {
        margin: 0 0 0.75rem 0;
        font-size: 0.9rem;
        color: #888;
        text-transform: uppercase;
    }

    .test-section .form-group {
        display: flex;
        gap: 0.5rem;
        margin: 0;
    }

    .test-section input {
        flex: 1;
    }

    .test-btn {
        padding: 0.75rem 1.5rem;
        background: #4a9eff;
        border: none;
        border-radius: 4px;
        color: #fff;
        cursor: pointer;
        font-weight: 500;
    }

    .test-btn:hover {
        background: #3a8eef;
    }

    .test-result {
        margin-top: 1rem;
        padding: 0.75rem;
        background: #2a1a1a;
        border: 1px solid #6b3333;
        border-radius: 4px;
        color: #ff9999;
    }

    .test-result.matched {
        background: #1a2a1a;
        border-color: #336b33;
        color: #99ff99;
    }

    /* Form Actions */
    .form-actions {
        display: flex;
        gap: 1rem;
        justify-content: flex-end;
        margin-top: 1.5rem;
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

    /* Table Styles */
    .rules-table {
        width: 100%;
        border-collapse: collapse;
        background: #0f3460;
        border: 1px solid #3a3a5c;
        border-radius: 6px;
        overflow: hidden;
    }

    .rules-table thead {
        background: #16213e;
    }

    .rules-table th {
        padding: 1rem;
        text-align: left;
        font-weight: 600;
        color: #888;
        text-transform: uppercase;
        font-size: 0.8rem;
        letter-spacing: 0.5px;
        border-bottom: 1px solid #3a3a5c;
    }

    .rules-table td {
        padding: 1rem;
        border-bottom: 1px solid #1a1a2e;
    }

    .rules-table tbody tr:hover {
        background: #1a3a52;
    }

    .pattern {
        font-family: 'Courier New', monospace;
        color: #4a9eff;
        font-size: 0.85rem;
    }

    .priority {
        text-align: center;
        font-weight: 600;
    }

    .action {
        font-weight: 600;
        text-align: center;
    }

    .action.allow {
        color: #66ff66;
    }

    .action.block {
        color: #ff6666;
    }

    .action.ask {
        color: #ffff66;
    }

    .enabled {
        text-align: center;
        color: #666;
    }

    .actions {
        display: flex;
        gap: 0.5rem;
        justify-content: center;
    }

    .edit-btn,
    .delete-btn {
        background: none;
        border: none;
        color: #888;
        cursor: pointer;
        padding: 0.25rem 0.5rem;
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .edit-btn:hover {
        color: #4a9eff;
    }

    .delete-btn:hover {
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

    @media (max-width: 768px) {
        .modal-content {
            width: 95%;
            max-height: 95vh;
        }

        .form-row {
            grid-template-columns: 1fr;
        }

        .rules-table {
            font-size: 0.85rem;
        }

        .rules-table th,
        .rules-table td {
            padding: 0.75rem 0.5rem;
        }

        .pattern {
            max-width: 150px;
            overflow: hidden;
            text-overflow: ellipsis;
        }
    }
</style>
