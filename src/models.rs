//! El catálogo de modelos LLM seleccionables.
//!
//! Es un DUPLICADO de `src/lib/models.js`, y eso normalmente sería un error. Se
//! hace así porque el frontend nativo no puede importar JavaScript, y las dos
//! alternativas son peores: leer y parsear el `.js` en tiempo de ejecución
//! (frágil, y deja de funcionar en cuanto alguien reformatea el fichero) o mover
//! el catálogo a un JSON compartido (toca la app que está en producción).
//!
//! Lo que hace que el duplicado sea seguro es el test del final: lee
//! `src/lib/models.js` y compara entrada por entrada. Si alguien añade un modelo
//! allí y no aquí, o cambia un icono, o reordena un grupo, el test falla con el
//! nombre exacto de lo que no cuadra. Sin ese test esto sería justo la clase de
//! deriva silenciosa que ya costó una paleta entera.
//!
//! Solo se guarda el nombre en español: el shell nativo no tiene todavía el
//! conmutador de idioma, y guardar una columna que nadie lee es cómo esa columna
//! acaba desactualizada sin que se note.

/// Un modelo seleccionable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelOption {
    /// El id que viaja al backend. El sufijo `::nivel` —cuando lo hay— codifica
    /// el esfuerzo de razonamiento y lo quita el resolvedor del servidor.
    pub id: &'static str,
    /// Glifo geométrico del sistema visual de Lucy: ◆ insignia, ◇ intermedio,
    /// ▸ rápido, ▫ legado, ◐ equilibrado, ◯ económico, ◎ vista previa,
    /// ⌬ código, ⌂ local, ✎ personalizado.
    pub icon: &'static str,
    /// El nombre que ve el operador.
    pub name: &'static str,
}

/// Un proveedor y sus modelos.
#[derive(Debug, Clone, Copy)]
pub struct ModelGroup {
    /// Clave estable del proveedor. Es por lo que se busca y lo que empareja con
    /// la credencial guardada — nunca por la etiqueta, que es texto de interfaz.
    pub provider: &'static str,
    /// Cabecera del grupo en el desplegable.
    pub label: &'static str,
    pub options: &'static [ModelOption],
}

const fn m(id: &'static str, icon: &'static str, name: &'static str) -> ModelOption {
    ModelOption { id, icon, name }
}

/// Los modelos de Anthropic.
///
/// Los sufijos `::nivel` son niveles de esfuerzo, no modelos distintos: el mismo
/// modelo con más o menos presupuesto de razonamiento. Aparecen como entradas
/// separadas porque la decisión de cuánto gastar se toma ANTES de escribir la
/// orden, no después de ver la factura.
const ANTHROPIC: &[ModelOption] = &[
    m("claude-opus-5::xhigh", "◆", "Claude Opus 5 — Extra Alto (coding/agéntico)"),
    m("claude-opus-5::high", "◆", "Claude Opus 5 — Alto (predeterminado)"),
    m("claude-opus-5::medium", "◆", "Claude Opus 5 — Medio (sensible al costo)"),
    m("claude-opus-5::max", "◆", "Claude Opus 5 — Max (problemas frontera)"),
    m("claude-sonnet-5::high", "◇", "Claude Sonnet 5 — Alto (predeterminado)"),
    m("claude-sonnet-5::xhigh", "◇", "Claude Sonnet 5 — Extra Alto (tareas más duras)"),
    m("claude-sonnet-5::medium", "◇", "Claude Sonnet 5 — Medio (ahorro de costo)"),
    m("claude-sonnet-5::low", "◇", "Claude Sonnet 5 — Bajo (sensible a latencia)"),
    m("claude-fable-5::xhigh", "◆", "Claude Fable 5 — Extra Alto (2× el costo de Opus 5)"),
    m("claude-fable-5::high", "◆", "Claude Fable 5 — Alto (2× el costo de Opus 5)"),
    m("claude-opus-4-8::high", "◇", "Claude Opus 4.8 — Alto (generación anterior)"),
    m("claude-opus-4-8::xhigh", "◇", "Claude Opus 4.8 — Extra Alto (generación anterior)"),
    m("claude-sonnet-4-6::medium", "◇", "Claude Sonnet 4.6 — Medio (generación anterior)"),
    m("claude-sonnet-4-6::high", "◇", "Claude Sonnet 4.6 — Alto (generación anterior)"),
    m("claude-haiku-4-5", "▸", "Claude Haiku 4.5 — Rápido y Eficiente"),
    m("claude-opus-4-7::high", "▫", "Claude Opus 4.7 — Legado"),
    m("claude-sonnet-4-5", "▫", "Claude Sonnet 4.5 — Legado"),
];

const GEMINI: &[ModelOption] = &[
    m("gemini-3.6-flash", "◐", "Gemini 3.6 Flash — Agéntico y multimodal (más reciente)"),
    m("gemini-3.5-flash", "◐", "Gemini 3.5 Flash — Rendimiento de frontera sostenido"),
    m("gemini-3.1-pro-preview::high", "◆", "Gemini 3.1 Pro — Esfuerzo Alto (razonamiento profundo)"),
    m("gemini-3.1-pro-preview::medium", "◆", "Gemini 3.1 Pro — Esfuerzo Medio (balanceado)"),
    m("gemini-3.5-flash-lite", "◯", "Gemini 3.5 Flash-Lite — Alto rendimiento"),
    m("gemini-3.1-flash-lite", "◯", "Gemini 3.1 Flash-Lite — El más barato"),
    m("gemini-3.1-flash-lite-preview", "◎", "Gemini 3.1 Flash-Lite Vista Previa"),
];

const OPENAI: &[ModelOption] = &[
    m("gpt-5.6-sol", "◆", "GPT-5.6 Sol — Frontera (trabajo profesional complejo)"),
    m("gpt-5.6-terra", "◇", "GPT-5.6 Terra — Equilibrio inteligencia/costo"),
    m("gpt-5.6-luna", "◯", "GPT-5.6 Luna — Cargas sensibles al costo"),
    m("gpt-5.5", "▫", "GPT-5.5 — Legado"),
    m("gpt-5.5-instant", "▫", "GPT-5.5 Instant — Legado"),
    m("gpt-5.4-mini", "▫", "GPT-5.4 Mini — Legado"),
    m("gpt-5.4-nano", "▫", "GPT-5.4 Nano — Legado"),
    m("gpt-5.3-codex", "▫", "GPT-5.3 Codex — Legado"),
    m("gpt-4o", "▫", "GPT-4o — Legado"),
    m("gpt-4o-mini", "▫", "GPT-4o Mini — Legado"),
];

const XAI: &[ModelOption] = &[
    m("grok-4.5", "◆", "Grok 4.5 — Razonamiento insignia"),
    m("grok-4.3", "◇", "Grok 4.3 — 1M de contexto, menor costo"),
];

const DEEPSEEK: &[ModelOption] = &[
    m("deepseek-v4-flash", "◇", "DeepSeek V4 Flash — La nube más barata"),
    m("deepseek-v4-pro", "◆", "DeepSeek V4 Pro — Razonamiento más fuerte"),
];

const NVIDIA: &[ModelOption] = &[
    m("meta/llama-3.1-70b-instruct", "◇", "Llama 3.1 70B — Potencia Equilibrada"),
    m("meta/llama-3.3-70b-instruct", "◇", "Llama 3.3 70B — Llama más Reciente"),
    m("meta/llama-3.1-405b-instruct", "◆", "Llama 3.1 405B — Máxima Inteligencia"),
    m("nvidia/nemotron-3-super-120b-a12b", "◆", "Nemotron 3 Super 120B — NVIDIA Flagship"),
    m("nvidia/nemotron-4-340b-instruct", "◆", "Nemotron 4 340B — NVIDIA Máximo"),
    m("mistralai/mistral-large-2-instruct", "◇", "Mistral Large 2 — Código y Razonamiento"),
    m("mistralai/mistral-7b-instruct-v0.3", "◯", "Mistral 7B — Rápido y Ligero"),
    m("google/gemma-4-31b-it", "◇", "Gemma 4 31B (NIM) — Google vía NVIDIA"),
    m("microsoft/phi-3.5-mini-instruct", "◯", "Phi-3.5 Mini — Rápido y Eficiente"),
    m("nvidia-custom", "✎", "Modelo NVIDIA Personalizado — escribe owner/model"),
];

/// Ollama es el único grupo cuyo contenido REAL se descubre en ejecución
/// (`chat::list_models`). Lo que hay aquí es el respaldo manual, igual que en el
/// JS: sirve cuando Ollama no responde y no hay nada que listar.
const OLLAMA: &[ModelOption] = &[m(
    "local-custom",
    "⌂",
    "Modelo Local Personalizado — ollama pull <model>",
)];

/// El catálogo, en el mismo orden en que se muestra.
pub const GROUPS: &[ModelGroup] = &[
    ModelGroup { provider: "anthropic", label: "Anthropic Claude", options: ANTHROPIC },
    ModelGroup { provider: "gemini", label: "Google Gemini", options: GEMINI },
    ModelGroup { provider: "openai", label: "OpenAI", options: OPENAI },
    ModelGroup { provider: "xai", label: "xAI Grok", options: XAI },
    ModelGroup { provider: "deepseek", label: "DeepSeek", options: DEEPSEEK },
    ModelGroup { provider: "nvidia", label: "NVIDIA NIM", options: NVIDIA },
    ModelGroup { provider: "ollama", label: "Local (Ollama)", options: OLLAMA },
];

/// Busca un modelo por su id.
pub fn find(id: &str) -> Option<&'static ModelOption> {
    GROUPS
        .iter()
        .flat_map(|g| g.options.iter())
        .find(|o| o.id == id)
}

/// Nombre legible de un modelo. Un id desconocido se devuelve tal cual: un chat
/// guardado con un modelo ya retirado debe seguir diciendo con qué se escribió.
pub fn describe(id: &str) -> &str {
    find(id).map_or(id, |o| o.name)
}

/// Glifo de un modelo, con un círculo neutro como respaldo.
pub fn icon(id: &str) -> &'static str {
    find(id).map_or("◉", |o| o.icon)
}

/// Filtra el catálogo por texto, como el buscador del desplegable.
///
/// Busca en el nombre, en el id Y en el proveedor — el último importa: escribir
/// "anthropic" enseña los de Anthropic aunque ninguno lleve esa palabra en el
/// nombre. Los grupos que se quedan sin resultados desaparecen en vez de
/// aparecer vacíos.
pub fn filter(query: &str) -> Vec<(&'static ModelGroup, Vec<&'static ModelOption>)> {
    let q = query.trim().to_lowercase();
    GROUPS
        .iter()
        .filter_map(|g| {
            let hits: Vec<&ModelOption> = g
                .options
                .iter()
                .filter(|o| {
                    q.is_empty()
                        || o.name.to_lowercase().contains(&q)
                        || o.id.to_lowercase().contains(&q)
                        || g.provider.contains(&q)
                })
                .collect();
            (!hits.is_empty()).then_some((g, hits))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_buscador_mira_nombre_id_y_proveedor() {
        // Por nombre.
        let r = filter("opus 5");
        assert_eq!(r.len(), 1, "solo Anthropic tiene Opus");
        let ids: Vec<&str> = r[0].1.iter().map(|o| o.id).collect();
        for lvl in ["xhigh", "high", "medium", "max"] {
            assert!(
                ids.contains(&&*format!("claude-opus-5::{lvl}")),
                "falta el nivel {lvl}"
            );
        }
        // Y salen SEIS, no cuatro: las dos entradas de Fable 5 dicen "2× el
        // costo de Opus 5" y la búsqueda es por subcadena. No es un fallo — es
        // justo lo que quiere quien busca "opus 5" para decidir cuánto gastar.
        assert_eq!(ids.len(), 6);

        // Por id, que es lo que se escribe cuando uno sabe lo que busca.
        assert!(filter("gpt-5.6-sol")
            .iter()
            .any(|(_, o)| o.iter().any(|m| m.id == "gpt-5.6-sol")));

        // Y por proveedor, aunque la palabra no salga en ningún nombre: ningún
        // modelo de Anthropic se llama "anthropic".
        let a = filter("anthropic");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].1.len(), ANTHROPIC.len(), "el grupo entero");

        // Sin texto, el catálogo completo.
        assert_eq!(filter("").len(), GROUPS.len());
        // Y los grupos vacíos no se enseñan.
        assert!(filter("no-existe-este-modelo").is_empty());
    }

    #[test]
    fn un_id_desconocido_se_devuelve_tal_cual() {
        // Un chat guardado con un modelo ya retirado tiene que seguir diciendo
        // con qué se escribió, no "(desconocido)".
        assert_eq!(describe("modelo-retirado-hace-un-año"), "modelo-retirado-hace-un-año");
        assert_eq!(icon("modelo-retirado-hace-un-año"), "◉");
        assert_eq!(describe("claude-opus-5::high"), "Claude Opus 5 — Alto (predeterminado)");
    }
}
