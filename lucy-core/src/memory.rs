//! Memoria NÚCLEO: los hechos que se inyectan en cada turno, y su decaimiento.
//!
//! Primer lote de `commands/memory.rs` movido al corazón compartido. Se eligió
//! éste porque es el grupo más autocontenido —el cálculo de decay es aritmética
//! pura y el renderizado ya estaba separado del acceso a DB— y porque es lo que
//! un frontend necesita para mostrar "qué recuerda Lucy de mí" sin depender de
//! Tauri.
//!
//! La app Tauri conserva sus `#[tauri::command]`, que ahora son envoltorios
//! delgados sobre esto. El atributo es registro IPC; el cuerpo no tiene por qué
//! saber que Tauri existe — de los 370 comandos del backend, 358 ya estaban en
//! esa situación.

use serde::{Deserialize, Serialize};

/// Por debajo de esto, la entrada NO se inyecta en el prompt.
pub const DECAY_INJECT_THRESHOLD: f32 = 0.30;
/// Por debajo de esto, se inyecta marcada con `[~aging~]` para que el modelo
/// matice en vez de afirmar.
pub const DECAY_AGING_THRESHOLD: f32 = 0.70;

/// Tope en caracteres del bloque CORE MEMORY completo.
///
/// ~1.5k tokens. No había ninguno: se concatenaba todo lo que sobreviviera al
/// decay, en CADA turno y por los dos constructores de prompt. El decay acota
/// la caducidad, no el volumen — cuarenta hechos fijados son cuarenta hechos a
/// score 1.0 para siempre, y el coste no aparece atribuido a la memoria en
/// ninguna vista.
pub const CORE_RENDER_MAX_CHARS: usize = 6_000;

/// Una fila de `memory_core`.
///
/// Sin derive de ts-rs, igual que antes del movimiento: este tipo nunca generó
/// binding y añadírselo solo produciría un fichero que nadie importa — más un
/// warning, porque ts-rs no sabe interpretar el `skip_serializing_if` de abajo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreMemoryEntry {
    pub id: String,
    pub section: String, // 'user_facts' | 'preferences' | 'rules' | 'environment' | 'identity' | 'context' | 'host'
    pub key: String,
    pub value: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// Calculado al leer, no almacenado. Las entradas fijadas devuelven 1.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decay_score: Option<f32>,
}

/// Vida media por sección, en días. Mayor = categoría más estable.
///
/// Varía porque los hechos de identidad (nombre, rol) cambian mucho más despacio
/// que los de contexto (proyecto actual). Las entradas fijadas se salen del
/// mecanismo entero.
fn decay_half_life_days(section: &str) -> f64 {
    // Normalizado: el frontend a veces capitaliza o usa español.
    let s = section.to_lowercase();
    match s.as_str() {
        "identity"    | "user_facts"  | "user"        => 365.0, // nombre, rol: anual
        "preference"  | "preferences" | "preferencia" => 180.0, // verboso/conciso, idioma
        "host"        | "hosts"       | "environment" => 90.0,  // hechos de la máquina
        "context"     | "project"     | "current"     => 60.0,  // trabajo activo: más rápido
        "rules"                                       => 730.0, // las reglas casi no caducan
        _                                             => 120.0, // por defecto ~4 meses
    }
}

/// Decaimiento exponencial por vida media: `score = 0.5 ^ (edad / vida_media)`.
///
/// Devuelve 1.0 para entradas fijadas y satura en 1.0 si `updated_at` está en el
/// futuro (seguridad ante desajuste de reloj).
pub fn compute_decay_score(updated_at: i64, section: &str, pinned: bool) -> f32 {
    if pinned {
        return 1.0;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age_secs = (now - updated_at).max(0);
    if age_secs == 0 {
        return 1.0;
    }
    let age_days = (age_secs as f64) / 86_400.0;
    let half_life = decay_half_life_days(section).max(1.0);
    let score = (0.5_f64).powf(age_days / half_life);
    score.clamp(0.0, 1.0) as f32
}

/// Etiqueta legible del score, para que la interfaz muestre de un vistazo qué
/// sigue creyéndose Lucy y sobre qué está matizando.
pub fn decay_label(score: f32) -> &'static str {
    if score >= DECAY_AGING_THRESHOLD {
        "fresh"
    } else if score >= DECAY_INJECT_THRESHOLD {
        "aging"
    } else {
        "stale"
    }
}

/// Renderiza el bloque CORE MEMORY listo para inyectar.
///
/// Puro: separado de la lectura de DB para que el filtro de decay, el
/// presupuesto, el orden de descarte, las dos notas y el determinismo se puedan
/// probar sin base de datos.
pub fn render_core_block(rows: &[CoreMemoryEntry]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let mut fresh: Vec<&CoreMemoryEntry> = Vec::new();
    let mut skipped_stale = 0usize;
    for r in rows {
        let score = r.decay_score.unwrap_or(1.0);
        if score < DECAY_INJECT_THRESHOLD {
            skipped_stale += 1;
            continue;
        }
        fresh.push(r);
    }

    // El presupuesto se aplica ANTES de agrupar: lo más fresco primero, empates
    // por (sección, clave) para que el mismo corpus rinda siempre el mismo
    // bloque. Esto es un PREFIJO de prompt — un bloque que se baraja entre
    // turnos también rompe el cacheo de prompt, que es la otra mitad de por qué
    // el determinismo importa aquí.
    fresh.sort_by(|a, b| {
        b.decay_score
            .unwrap_or(1.0)
            .partial_cmp(&a.decay_score.unwrap_or(1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.section.cmp(&b.section))
            .then_with(|| a.key.cmp(&b.key))
    });
    let mut budget = CORE_RENDER_MAX_CHARS;
    let mut dropped_for_budget = 0usize;
    let mut kept: Vec<&CoreMemoryEntry> = Vec::new();
    for r in fresh {
        // +6 cubre la sangría, " = " y el salto de línea; el marcador se cuenta
        // donde aplica.
        let cost = r.key.len()
            + r.value.len()
            + 6
            + if r.decay_score.unwrap_or(1.0) < DECAY_AGING_THRESHOLD { 9 } else { 0 };
        if cost > budget {
            dropped_for_budget += 1;
            continue;
        }
        budget -= cost;
        kept.push(r);
    }

    let mut by_section: std::collections::BTreeMap<String, Vec<&CoreMemoryEntry>> =
        std::collections::BTreeMap::new();
    for r in kept {
        by_section.entry(r.section.clone()).or_default().push(r);
    }
    if by_section.is_empty() {
        // Todo caducado — devolver una pista para que el modelo pregunte en vez
        // de alucinar valores por defecto.
        return format!(
            "--- CORE MEMORY ---\n\
             (No fresh facts available — {} stale entries were filtered. \
             If the user references personal info, ask them to confirm rather \
             than assuming defaults.)\n\
             --- END CORE MEMORY ---",
            skipped_stale,
        );
    }

    let mut out = String::from("--- CORE MEMORY (always-on facts) ---\n");
    for (section, items) in &by_section {
        out.push_str(&format!("[{}]\n", section));
        for it in items {
            let score = it.decay_score.unwrap_or(1.0);
            let marker = if score < DECAY_AGING_THRESHOLD { " [~aging~]" } else { "" };
            out.push_str(&format!("  {} = {}{}\n", it.key, it.value, marker));
        }
    }
    if skipped_stale > 0 {
        out.push_str(&format!(
            "(Note: {} additional fact{} were filtered as stale. If relevant, ask the user to confirm.)\n",
            skipped_stale,
            if skipped_stale == 1 { "" } else { "s" }
        ));
    }
    if dropped_for_budget > 0 {
        // Nota SEPARADA de la de caducados, y tiene que existir: un modelo que
        // cree haber recibido todos los hechos núcleo responde desde un conjunto
        // incompleto sin matizar — una respuesta segura y equivocada sobre la
        // propia configuración del usuario.
        out.push_str(&format!(
            "(Note: {} further fact{} omitted for space. Core memory is over budget — \
             ask the user rather than assuming, and suggest they prune or pin what matters.)\n",
            dropped_for_budget,
            if dropped_for_budget == 1 { "" } else { "s" }
        ));
    }
    out.push_str("--- END CORE MEMORY ---");
    out
}

/// Lee `memory_core` y devuelve el bloque renderizado. Cadena vacía si no hay
/// filas o si la DB no está disponible — este bloque no debe poder tumbar un
/// turno.
pub fn render_core() -> String {
    let rows_res: Result<Vec<CoreMemoryEntry>, String> = crate::with_db(|conn| {
        // Sin `WHERE pinned = 1`: el decay se ocupa también de las no fijadas.
        // Las fijadas puntúan 1.0 siempre, así que no les afecta.
        let mut stmt = match conn.prepare(
            "SELECT id, section, key, value, pinned, created_at, updated_at
             FROM memory_core ORDER BY section, key",
        ) {
            Ok(s) => s,
            Err(_) => return Ok(Vec::new()),
        };
        let rows: Vec<CoreMemoryEntry> = stmt
            .query_map([], |r| {
                let section: String = r.get(1)?;
                let pinned: bool = r.get::<_, i64>(4)? != 0;
                let updated_at: i64 = r.get(6)?;
                Ok(CoreMemoryEntry {
                    id: r.get(0)?,
                    section: section.clone(),
                    key: r.get(2)?,
                    value: r.get(3)?,
                    pinned,
                    created_at: r.get(5)?,
                    updated_at,
                    decay_score: Some(compute_decay_score(updated_at, &section, pinned)),
                })
            })
            .map(|iter| iter.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        Ok(rows)
    });
    render_core_block(&rows_res.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(section: &str, key: &str, value: &str, score: f32) -> CoreMemoryEntry {
        CoreMemoryEntry {
            id: format!("{section}/{key}"),
            section: section.into(),
            key: key.into(),
            value: value.into(),
            pinned: score >= 1.0,
            created_at: 0,
            updated_at: 0,
            decay_score: Some(score),
        }
    }

    #[test]
    fn a_small_corpus_renders_whole_and_says_nothing_about_budgets() {
        let rows = vec![
            entry("user_facts", "nombre", "Iván", 1.0),
            entry("environment", "os", "Windows 11", 0.9),
        ];
        let out = render_core_block(&rows);
        assert!(out.contains("nombre = Iván"), "{out}");
        assert!(out.contains("os = Windows 11"), "{out}");
        assert!(!out.contains("omitted for space"), "no budget note expected: {out}");
    }

    #[test]
    fn the_block_stays_under_budget_and_says_so_when_it_truncates() {
        let rows: Vec<CoreMemoryEntry> = (0..400)
            .map(|i| entry("user_facts", &format!("k{i:03}"), &"v".repeat(60), 0.9))
            .collect();
        let out = render_core_block(&rows);
        assert!(
            out.len() < CORE_RENDER_MAX_CHARS + 500,
            "block ran to {} chars, budget is {}",
            out.len(),
            CORE_RENDER_MAX_CHARS
        );
        assert!(out.contains("omitted for space"), "truncation must be declared: {out}");
    }

    #[test]
    fn pinned_facts_survive_the_cut_and_the_stalest_go_first() {
        let mut rows: Vec<CoreMemoryEntry> = (0..300)
            .map(|i| entry("context", &format!("filler{i:03}"), &"x".repeat(60), 0.35))
            .collect();
        rows.push(entry("identity", "pinned_fact", "no debe caerse", 1.0));
        let out = render_core_block(&rows);
        assert!(out.contains("pinned_fact = no debe caerse"), "pinned entry was dropped: {out}");
        assert!(out.contains("omitted for space"));
    }

    #[test]
    fn truncation_is_announced_separately_from_staleness() {
        let mut rows: Vec<CoreMemoryEntry> = (0..300)
            .map(|i| entry("context", &format!("k{i:03}"), &"y".repeat(60), 0.8))
            .collect();
        rows.push(entry("context", "olvidado", "hace mucho", 0.05));
        let out = render_core_block(&rows);
        assert!(out.contains("filtered as stale"), "{out}");
        assert!(out.contains("omitted for space"), "{out}");
    }

    #[test]
    fn the_same_corpus_always_renders_the_same_block() {
        let rows: Vec<CoreMemoryEntry> = (0..200)
            .map(|i| entry("user_facts", &format!("k{i:03}"), &"z".repeat(60), 0.7))
            .collect();
        let first = render_core_block(&rows);
        for _ in 0..10 {
            assert_eq!(render_core_block(&rows), first, "render is not deterministic");
        }
    }

    #[test]
    fn pinned_entries_never_decay_and_stale_ones_do() {
        // Hace un año, pero fijada.
        let year_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
            - 365 * 86_400;
        assert_eq!(compute_decay_score(year_ago, "context", true), 1.0);
        // La misma antigüedad sin fijar, en una sección de vida media 60 días:
        // ~6 vidas medias → prácticamente cero.
        let s = compute_decay_score(year_ago, "context", false);
        assert!(s < 0.05, "esperaba decaimiento fuerte, dio {s}");
        assert_eq!(decay_label(s), "stale");
    }
}
