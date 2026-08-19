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

/// Qué es un fichero adjunto y qué se puede hacer con él. La decisión —esto es
/// texto, esto una imagen, esto no se puede mandar y por esto— no es de interfaz.
pub mod attach;
/// El texto de un PDF adjunto. La ingesta RAG sigue en `commands/pdf.rs`.
pub mod pdf;
/// Qué se deja correr solo y qué no. Aparece con el bucle automático: mientras
/// una persona leyera cada comando, el guardrail era esa persona.
pub mod guard;
/// El workspace del agente: plan, ejecución, trace y artefactos. Port de
/// `src/lib/cockpit/agent-workspace.ts` — el modelo, y sobre todo sus topes.
pub mod agent;
/// Comandos que no se deshacen. Otra pregunta que `guard`: éste no busca
/// ataques, busca lo irreversible.
pub mod destructive;
/// Sub-agentes. `agent::AgentFork` llevaba desde el principio con su estado de
/// cuatro valores y su contador de «corriendo», y nada los creaba. Aquí está lo
/// que faltaba — y la decisión de que un sub-agente SOLO LEA.
pub mod forks;
/// Elevación a administrador con UAC. El mecanismo que faltaba en la V1 y la V2.
pub mod elevate;
pub mod chat;
/// Los proveedores de nube. Sin esto, el catálogo de modelos ofrece cincuenta y
/// uno y solo funcionan los locales.
pub mod cloud;
/// El registro de equipos remotos. El MISMO índice que la app, byte a byte.
pub mod hosts;
/// Las claves de API. Se podían LEER y no escribir: en una instalación limpia,
/// egui arrancaba sin poder hablar con nadie y sin decir dónde arreglarlo.
pub mod keys;
/// Lectura de la cola de un log. El MECANISMO; la política de rutas se queda
/// en quien expone el comando.
pub mod logs;
/// El registro de auditoría. La tabla la creó la app Tauri y el shell nativo
/// ejecutaba comandos sin dejar constancia de ninguno.
pub mod audit;
/// Escribir memorias, con la deduplicación en la puerta. Era la mitad que le
/// faltaba entera al shell nativo: leía el corpus y no podía añadirle una fila.
pub mod memories;
/// Los principios: reglas que Lucy aplica SIEMPRE, no cuando vienen al caso.
/// Por eso no pasan por la búsqueda semántica — ahí solo aparecerían cuando ya
/// no hacían falta.
pub mod principles;
/// Cristales: el resumen de una sesión entera. Lo que una memoria por turno no
/// puede contestar, porque aquello no fue un turno sino once.
pub mod crystals;
/// Insights: el patrón que se repite entre memorias que nadie escribió juntas.
/// Agrupa por CONTENIDO y no por etiquetas — ver la cabecera del módulo.
pub mod insights;
/// El calendario del mantenimiento, por vencimiento persistido y no por
/// temporizador: un `sleep(48h)` no despierta nunca en un portátil que se cierra
/// cada tarde.
pub mod maintenance;
/// Documentos ingeridos. La pieza de la que sale gran parte de la memoria, y
/// cuyo último eslabón es que `pdf_search` esté en el catálogo de herramientas.
pub mod docs;
/// Inventario de un equipo: puertos, servicios, software, certificados, tareas.
/// Sin JSON fabricado a mano, que es lo que tumbaba el inventario entero.
pub mod inventory;
/// Qué ha cambiado desde que dijimos que el equipo estaba bien. Los puertos
/// efímeros no cuentan: son la diferencia entre un informe y un montón de ruido.
pub mod drift;
/// Checks de CIS Benchmark. El catálogo se COMPARTE con la app por `include_str!`
/// en vez de copiarse, y la evidencia llega entera en vez de con puntos.
pub mod compliance;
/// Fundir memorias que dicen lo mismo. Estaba escrito y nadie lo llamaba nunca.
pub mod consolidate;
/// Lo que distingue a NexShell de una consola: decidir si lo escrito es un
/// comando o una frase, y limpiar lo que el modelo devuelve.
pub mod nexshell;
/// Memoria NÚCLEO + decaimiento. Primer lote de `commands/memory.rs`.
pub mod memory;
/// El catálogo de modelos LLM. Duplica `src/lib/models.js` a propósito, con un
/// test que compara los dos ficheros para que el duplicado no pueda derivar.
pub mod models;
/// Lo que cuesta cada turno. Duplica `src/lib/model-pricing.ts`, con test.
pub mod pricing;
/// El prompt de sistema, por secciones. Port de la ARQUITECTURA de
/// `commands/prompt_sections.rs`, no de su texto: allí se describen herramientas
/// que este shell todavía no tiene.
pub mod prompt;
/// Enrutado: avisa cuando el modelo elegido se queda corto para lo que se pide.
/// NO cambia el modelo por su cuenta — ver la cabecera del módulo.
/// La historia de los escaneos de compliance. El escaneo dice qué cumple; esto
/// dice QUÉ SE HA ROTO, que es la pregunta de verdad — un control que ya fallaba
/// es deuda conocida, uno que pasaba y hoy falla es una noticia.
pub mod posture;
pub mod routing;
/// Los atajos de la pantalla vacía, escritos por un modelo local pequeño a
/// partir del estado real del equipo. Los de fábrica son genéricos y envejecen
/// mal: con dos servicios caídos, «Salud del sistema» tapa la respuesta.
pub mod suggest;
/// Ponerle nombre a una pestaña con un modelo local, o con el de nube más
/// barato, o —si no hay ninguno— recortando la orden por palabras enteras.
pub mod titles;
/// Cuidados de la base: copiarla, contar qué hay dentro y quitar lo que sobra.
/// Enseñar dónde vive algo irreemplazable sin ofrecer copiarlo es media
/// instrucción.
pub mod upkeep;
/// Las conversaciones abiertas, para que sobrevivan al cierre.
pub mod session;
/// Capturar la pantalla. La mitad de VER de `local_screen.rs`; conducir el
/// escritorio no viene, y no por falta de tiempo.
pub mod screen;
/// Lo que Lucy sabe del operador entre sesiones. El hueco del prompt existía y
/// viajaba vacío; la etiqueta con la que se escribe se ignoraba.
pub mod profile;
/// Los skills: instrucciones que Lucy carga cuando vienen al caso. Ficheros en
/// disco, no objetos compilados — se añade uno dejando un `SKILL.md`.
pub mod skills;
/// PowerShell y decodificación de consola. Base de Inventario, Compliance y
/// NexShell — ninguna migra sin esto.
pub mod shell;
pub mod system;
/// Las herramientas de LECTURA que Lucy puede pedir. Antes se anotaban y no se
/// cumplían, que parece que funcionó.
pub mod tools;
/// La conversación que viaja al modelo, y su recorte.
pub mod turns;
/// Las etiquetas de acción de una respuesta de Lucy. Detecta, no ejecuta.
pub mod tags;
/// Ranking semántico: blobs, coseno y los filtros que deciden qué fila puede
/// compararse con qué consulta. El transporte HTTP se queda en cada frontend.
pub mod vectors;

pub type DbPool = r2d2::Pool<SqliteConnectionManager>;
static POOL: OnceCell<DbPool> = OnceCell::new();

/// Una memoria de largo plazo — misma forma que la fila `agent_memories`.
///
/// LA definición, no una de dos: `src-tauri` la reexporta desde aquí. Existía
/// duplicada campo por campo, y una struct copiada es una struct que acaba
/// difiriendo en una columna que alguien añadió a un lado.
///
/// El derive de ts-rs va tras la feature `ts` porque exportar TypeScript es un
/// problema de la app Tauri (sus tipos cruzan el puente IPC). El shell nativo
/// llama funciones Rust directamente y no debe compilar ts-rs por compartir un
/// tipo. La ruta de exportación resuelve al mismo `src/lib/types/` desde
/// cualquiera de los dos crates.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "../src/lib/types/"))]
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

/// Adopta un pool YA construido en vez de abrir uno propio.
///
/// Es lo que usa la app Tauri. Sin esto, enlazar este crate desde `src-tauri`
/// significaría **dos** pools r2d2 sobre el mismo fichero SQLite: el doble de
/// conexiones, dos juegos de PRAGMA que pueden discrepar, y contención de
/// escritura entre dos mitades del mismo proceso. Un pool, dos consumidores.
///
/// El shell nativo sigue usando `init(path)`, que sí construye el suyo — ahí no
/// hay ninguno que adoptar.
///
/// Idempotente: si ya hay pool, no hace nada y devuelve `Ok`. Eso importa
/// porque el arranque de Tauri puede reentrar.
pub fn init_with_pool(pool: DbPool) -> Result<(), String> {
    if POOL.get().is_some() {
        return Ok(());
    }
    POOL.set(pool).map_err(|_| "lucy-core: pool ya inicializado".to_string())
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

/// Memorias vivas (no-pdf, no-superseded) más recientes.
///
/// Ésta ES la consulta que corre el backend Tauri: `metrics::get_recent_memories`
/// delega aquí. Antes decía "la MISMA consulta" siendo una copia, y ya habían
/// derivado — el tope estaba en 300 aquí y en 50 allí. Nadie lo notó porque
/// nada llamaba todavía a esta versión.
///
/// Las dos exclusiones del `WHERE` son deliberadas y costaron bugs:
///   • `superseded_by` — sin ella la pestaña Verify volvía a detectar conflictos
///     que el usuario acababa de resolver (v1.6.13).
///   • `session_id NOT LIKE 'pdf:%'` — un manual ingerido escribe 1000+ filas de
///     importancia 2 de golpe y saturaba la ventana, inundando el navegador de
///     memoria Y la inyección de contexto ambiental (v1.7.233). Los trozos de
///     documento siguen siendo alcanzables por pdf_search.
///
/// El tope de 50 es el de la app: es lo que se ha enviado siempre, y la ventana
/// que la inyección de contexto asume por turno.
pub fn get_recent_memories(limit: Option<i64>) -> Result<Vec<AgentMemory>, String> {
    with_db(|conn| {
        let lim = limit.unwrap_or(15).max(1).min(50);
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
