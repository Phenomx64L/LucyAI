//! lucy-core — el corazón SIN-TAURI de Lucy.
//!
//! Compartido por la app Tauri (`src-tauri`) y el shell nativo egui (`lucy-egui`).
//! Cero UI, cero Tauri, cero WebView: solo el pool de la DB + la lógica de dominio
//! que sobrevive a cualquier toolkit gráfico.
//!
//! **Este crate es lo que permite a Lucy dejar el WebView**: el binario egui
//! enlaza ESTO (no `tauri`), así que no arrastra ningún motor de navegador.
//!
//! Slice 1 (migración WebView, v1.7.236): pool + `AgentMemory` + lecturas de
//! memoria. Abre la lucy.db EXISTENTE (la app Tauri sigue siendo dueña de la
//! creación del esquema). Los siguientes slices moverán aquí el resto de la
//! lógica pura del backend (comandos que no usan `AppHandle`/`State`).

use once_cell::sync::OnceCell;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod chat;
pub mod system;

pub type DbPool = r2d2::Pool<SqliteConnectionManager>;
static POOL: OnceCell<DbPool> = OnceCell::new();

/// Una memoria de largo plazo — misma forma que la fila `agent_memories`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    pub id: i64,
    pub session_id: String,
    pub title: String,
    pub content: String,
    pub tags: String,       // JSON array como string (compat SQLite)
    pub files: String,      // JSON array
    pub importance: i64,    // 1-3
    pub created_at: i64,    // unix epoch segundos
}

/// Abre el pool compartido sobre una lucy.db EXISTENTE (la app Tauri crea el
/// esquema). Idempotente. Sin `AppHandle` — solo una ruta de archivo.
pub fn init(path: &Path) -> Result<(), String> {
    if POOL.get().is_some() {
        return Ok(());
    }
    let manager = SqliteConnectionManager::file(path).with_init(|c| {
        c.execute_batch(
            "PRAGMA journal_mode=WAL;\
             PRAGMA synchronous=NORMAL;\
             PRAGMA busy_timeout=5000;\
             PRAGMA foreign_keys=ON;",
        )
    });
    let pool = r2d2::Pool::builder()
        .max_size(4)
        .build(manager)
        .map_err(|e| format!("lucy-core pool: {e}"))?;
    POOL.set(pool).map_err(|_| "lucy-core: pool ya inicializado".to_string())?;
    Ok(())
}

/// Toma prestada una conexión del pool. Mismo contrato que el `with_db` de la app.
pub fn with_db<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&rusqlite::Connection) -> Result<R, String>,
{
    let pool = POOL.get().ok_or("lucy-core: DB no inicializada")?;
    let conn = pool.get().map_err(|e| format!("pool.get: {e}"))?;
    f(&conn)
}

/// Memorias vivas (no-pdf, no-superseded) más recientes — la MISMA consulta que
/// corre `get_recent_memories` en el backend Tauri.
pub fn get_recent_memories(limit: Option<i64>) -> Result<Vec<AgentMemory>, String> {
    with_db(|conn| {
        let lim = limit.unwrap_or(15).max(1).min(300);
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, title, content, tags, files, importance, created_at \
                 FROM agent_memories \
                 WHERE (superseded_by IS NULL OR superseded_by = '') \
                   AND session_id NOT LIKE 'pdf:%' \
                 ORDER BY importance DESC, created_at DESC LIMIT ?1",
            )
            .map_err(|e| format!("get_recent prepare: {e}"))?;
        let rows = stmt
            .query_map([lim], |r| {
                Ok(AgentMemory {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    title: r.get(2)?,
                    content: r.get(3)?,
                    tags: r.get(4)?,
                    files: r.get(5)?,
                    importance: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })
            .map_err(|e| format!("get_recent query: {e}"))?;
        Ok(rows.flatten().collect())
    })
}
