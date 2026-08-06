//! El modelo de datos del workspace del agente — los cuatro paneles del cockpit.
//!
//! Port de `src/lib/cockpit/agent-workspace.ts`. Plan, Ejecución, Trace y
//! Artefactos se dibujan PURAMENTE desde estas estructuras, así que quien las
//! llene —el bucle del agente hoy, otro frontend mañana— mueve la misma
//! interfaz. Aquí no hay lógica de agente: es re-exponer estado que ya existe.
//!
//! LO QUE DE VERDAD VIVE AQUÍ SON LOS LÍMITES. Cada carril tiene un tope de
//! entradas y de tamaño, y esos números no son redondeos bonitos: salen de con
//! qué se compara cada carril, y su ausencia fue un problema real en la versión
//! web. Se explican uno a uno más abajo, junto a la constante.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milisegundos desde el epoch. Una sola función para que todas las marcas de
/// tiempo del workspace midan lo mismo.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepStatus {
    #[default]
    Pending,
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct PlanStep {
    pub id: String,
    pub label: String,
    pub status: StepStatus,
    /// El comando o la herramienta que ejecuta el paso.
    pub detail: String,
    /// Dónde corre — el equipo local o uno remoto.
    pub host: String,
    /// Duración, una vez terminado.
    pub ms: Option<u64>,
    pub ts: u64,
    /// Por qué este paso necesita que lo mire una persona, si lo necesita.
    ///
    /// `None` es lo normal y significa que puede correr solo cuando la pestaña
    /// está en automático. Lo pone el guardrail —no la interfaz— porque «esto
    /// puede correr sin que nadie lo lea» es una propiedad del paso, no de cómo
    /// se pinte: la misma decisión vale igual si mañana la toma otro frontend.
    ///
    /// Un paso BLOQUEADO no usa este campo: nace en `Error` y no tiene botón.
    /// Esto es para el estado del medio — plausible, delicado, decide un humano.
    pub needs_human: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ExecEntry {
    pub id: String,
    pub cmd: String,
    pub output: String,
    pub ok: bool,
    pub ms: Option<u64>,
    /// PS / CMD / SSH…
    pub engine: String,
    /// Código de salida deducido. `None` = ninguno.
    pub code: Option<i32>,
    pub ts: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TraceEntry {
    pub id: String,
    /// think / act / react / info…
    pub phase: String,
    pub label: String,
    pub detail: String,
    pub step: Option<u32>,
    pub ts: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Edit,
    Write,
    Skill,
    Memory,
}

impl ArtifactKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Edit => "editado",
            Self::Write => "escrito",
            Self::Skill => "skill",
            Self::Memory => "memoria",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Artifact {
    pub id: String,
    pub kind: ArtifactKind,
    pub path: String,
    pub summary: String,
    /// Contenido anterior y nuevo — con esto se dibuja el diff.
    pub before: String,
    pub after: String,
    pub ts: u64,
    /// Si el cambio ya está en el disco.
    ///
    /// UN ARTEFACTO NACE SIN APLICAR. Lo que hay en `after` es lo que Lucy
    /// propone escribir, no lo que hay escrito, y esa diferencia es la razón de
    /// que este campo exista: hasta hoy el carril enseñaba «propuesto — sin
    /// escribir» en el resumen, que es una forma de decirlo que no se puede
    /// consultar desde el código. Ahora el botón mira aquí.
    pub applied: bool,
    /// Por qué no se puede aplicar, cuando no se puede. Vacío = se puede.
    ///
    /// Se guarda en vez de descartar la propuesta: un `editfile` cuyo texto
    /// viejo no aparece es un error de Lucy que ella tiene que ver para
    /// corregirlo, no algo que deba desaparecer sin dejar rastro.
    pub blocked: String,
}

/// Estado de un sub-agente lanzado con `fork_task`.
///
/// `Collected` es un estado DISTINTO de `Done` a propósito. Un fork termina por
/// su cuenta, pero su resultado no vuelve a la tarea principal hasta que un
/// `wait_task` posterior lo recoge — y el hueco entre esos dos momentos es
/// justo lo que el operador necesita ver para entender por qué Lucy sigue
/// trabajando.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkStatus {
    Running,
    Done,
    Error,
    Collected,
}

#[derive(Debug, Clone)]
pub struct AgentFork {
    /// El id que viene de la llamada a la herramienta, NO uno generado: el bucle
    /// y la conversación se refieren al fork por ese nombre.
    pub id: String,
    pub instruction: String,
    pub model: String,
    pub status: ForkStatus,
    pub result: String,
    pub ms: Option<u64>,
    pub ts: u64,
}

#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    pub running: bool,
    /// Paso actual, contando desde 1.
    pub step_index: u32,
    pub step_total: u32,
    pub host: String,
    pub model: String,
    pub cost_usd: f64,
}

// ── Límites ──────────────────────────────────────────────────────────────────
//
// En la versión web estos dos carriles nacieron SIN tope, y son los que llevan
// las cargas más grandes.
//
// `exec_push` guarda la salida cruda de un comando. La misma cadena se recorta a
// 4 000 caracteres para la burbuja de conversación y a 16 000 para el modelo —
// en la misma línea— y se guardaba entera. Un listado recursivo o un volcado de
// log son megabytes, y `reset` solo limpia al EMPEZAR una tarea: una sesión de
// sesenta turnos acumulaba cada byte de cada comando.
//
// `trace_push` es peor en naturaleza: es el ESPEJO de `liveTrace`, cuyo propio
// almacén corta en 2 000 entradas con un comentario calculando la memoria.
// Copiar a un array sin tope anula ese buffer circular en silencio — el origen
// recorta y la copia nunca.
//
// Los topes se eligen contra aquello con lo que cada carril se compara: la
// salida de comando a los 16 000 que ya recibe el modelo, y el trace a las 2 000
// de `liveTrace` para que los dos vayan al paso.
const EXEC_MAX_ENTRIES: usize = 200;
const EXEC_MAX_OUTPUT: usize = 16_000;
const TRACE_MAX_ENTRIES: usize = 2_000;
const TRACE_MAX_DETAIL: usize = 4_000;
const ARTIFACT_MAX_ENTRIES: usize = 60;
const ARTIFACT_MAX_BODY: usize = 8_000;

/// Recorta a `max` CARACTERES y lo dice.
///
/// Por caracteres y no por bytes: cortar un `String` de Rust por un índice de
/// byte que cae en medio de un carácter multibyte es un pánico, y la salida de
/// un comando en español lleva acentos en cada línea. El original en JavaScript
/// no tenía este problema y por eso no lo menciona; aquí es la diferencia entre
/// truncar y tirar la aplicación.
///
/// La marca es visible a propósito: una salida cortada sin avisar se lee como
/// una salida completa, y esa es la clase de mentira que hace depurar a ciegas.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}\n… (truncado)")
}

/// El workspace completo: los cinco carriles y el estado del turno.
#[derive(Debug, Default)]
pub struct Workspace {
    pub plan: Vec<PlanStep>,
    pub exec: Vec<ExecEntry>,
    pub trace: Vec<TraceEntry>,
    pub artifacts: Vec<Artifact>,
    pub forks: Vec<AgentFork>,
    pub status: AgentStatus,
    seq: u64,
}

impl Workspace {
    fn nid(&mut self) -> String {
        self.seq += 1;
        format!("w{}", self.seq)
    }

    /// Vacía todos los carriles — al empezar una tarea nueva.
    ///
    /// El contador de ids NO se reinicia: dos entradas de dos tareas distintas
    /// con el mismo id confundirían a cualquier cosa que guarde una referencia.
    pub fn reset(&mut self) {
        self.plan.clear();
        self.exec.clear();
        self.trace.clear();
        self.artifacts.clear();
        self.forks.clear();
        self.status = AgentStatus::default();
    }

    /// ¿Hay algo que enseñar? Gobierna las herramientas de exportar y limpiar.
    pub fn is_empty(&self) -> bool {
        self.plan.is_empty()
            && self.exec.is_empty()
            && self.trace.is_empty()
            && self.artifacts.is_empty()
    }

    pub fn plan_set(&mut self, steps: Vec<PlanStep>) {
        self.plan = steps;
    }

    /// Modifica un paso por id. Devuelve si lo encontró — un `false` significa
    /// que el bucle y el panel están hablando de pasos distintos.
    pub fn plan_update(&mut self, id: &str, status: StepStatus, ms: Option<u64>) -> bool {
        if let Some(s) = self.plan.iter_mut().find(|s| s.id == id) {
            s.status = status;
            if ms.is_some() {
                s.ms = ms;
            }
            return true;
        }
        false
    }

    pub fn plan_append(&mut self, mut step: PlanStep) -> String {
        let id = self.nid();
        step.id = id.clone();
        if step.ts == 0 {
            step.ts = now_ms();
        }
        self.plan.push(step);
        id
    }

    pub fn exec_push(&mut self, mut e: ExecEntry) {
        e.id = self.nid();
        e.output = clip(&e.output, EXEC_MAX_OUTPUT);
        if e.ts == 0 {
            e.ts = now_ms();
        }
        self.exec.push(e);
        trim_front(&mut self.exec, EXEC_MAX_ENTRIES);
    }

    pub fn trace_push(&mut self, mut t: TraceEntry) {
        t.id = self.nid();
        t.detail = clip(&t.detail, TRACE_MAX_DETAIL);
        if t.ts == 0 {
            t.ts = now_ms();
        }
        self.trace.push(t);
        trim_front(&mut self.trace, TRACE_MAX_ENTRIES);
    }

    pub fn artifact_push(&mut self, mut a: Artifact) {
        a.id = self.nid();
        a.before = clip(&a.before, ARTIFACT_MAX_BODY);
        a.after = clip(&a.after, ARTIFACT_MAX_BODY);
        if a.ts == 0 {
            a.ts = now_ms();
        }
        self.artifacts.push(a);
        trim_front(&mut self.artifacts, ARTIFACT_MAX_ENTRIES);
    }

    /// Registra un fork en cuanto se lanza, para que se vea mientras corre.
    ///
    /// Relanzar un id conocido REEMPLAZA la fila en vez de duplicarla: el bucle
    /// ya rechaza ids repetidos, así que una repetición aquí es un reintento.
    pub fn fork_start(&mut self, id: &str, instruction: &str, model: &str) {
        self.forks.retain(|f| f.id != id);
        self.forks.push(AgentFork {
            id: id.to_string(),
            instruction: instruction.to_string(),
            model: model.to_string(),
            status: ForkStatus::Running,
            result: String::new(),
            ms: None,
            ts: now_ms(),
        });
    }

    /// Cierra un fork. La duración se deriva de la hora de lanzamiento guardada
    /// y no se acepta del llamante, para que todas las filas midan lo mismo.
    pub fn fork_finish(&mut self, id: &str, status: ForkStatus, result: &str) {
        if let Some(f) = self.forks.iter_mut().find(|f| f.id == id) {
            f.status = status;
            f.result = result.to_string();
            f.ms = Some(now_ms().saturating_sub(f.ts));
        }
    }

    /// Marca un fork terminado como ya reincorporado por `wait_task`.
    pub fn fork_collected(&mut self, id: &str) {
        if let Some(f) = self.forks.iter_mut().find(|f| f.id == id) {
            f.status = ForkStatus::Collected;
        }
    }

    /// Sub-agentes en curso. El panel de Plan lleva esta señal para que un fork
    /// corriendo se vea desde cualquier pestaña.
    pub fn forks_running(&self) -> usize {
        self.forks
            .iter()
            .filter(|f| f.status == ForkStatus::Running)
            .count()
    }
}

/// Deja solo las últimas `max` entradas. Se recorta por delante porque lo
/// reciente es lo que se está mirando.
fn trim_front<T>(v: &mut Vec<T>, max: usize) {
    if v.len() > max {
        v.drain(..v.len() - max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(output: &str) -> ExecEntry {
        ExecEntry {
            cmd: "Get-Process".into(),
            output: output.into(),
            ok: true,
            ..Default::default()
        }
    }

    #[test]
    fn la_salida_de_un_comando_se_recorta_y_lo_dice() {
        let mut w = Workspace::default();
        w.exec_push(exec(&"x".repeat(EXEC_MAX_OUTPUT + 500)));
        let o = &w.exec[0].output;
        assert!(o.ends_with("… (truncado)"), "una salida cortada sin avisar se lee como completa");
        assert_eq!(o.chars().count(), EXEC_MAX_OUTPUT + "\n… (truncado)".chars().count());
    }

    #[test]
    fn recortar_por_caracteres_no_parte_un_acento() {
        // El caso que en Rust es un pánico y en JavaScript no existía: cortar
        // por índice de byte en medio de un carácter multibyte. La salida de un
        // comando en español lleva acentos en casi cada línea.
        let mut w = Workspace::default();
        let ñ = "ñ".repeat(EXEC_MAX_OUTPUT + 10);
        w.exec_push(exec(&ñ));
        assert!(w.exec[0].output.starts_with('ñ'));
        assert!(w.exec[0].output.ends_with("… (truncado)"));
    }

    #[test]
    fn los_carriles_grandes_tienen_tope() {
        let mut w = Workspace::default();
        for i in 0..EXEC_MAX_ENTRIES + 50 {
            w.exec_push(exec(&format!("salida {i}")));
        }
        assert_eq!(w.exec.len(), EXEC_MAX_ENTRIES);
        // Y lo que se conserva es lo ÚLTIMO: en un run largo, lo que se está
        // mirando es lo que acaba de pasar.
        assert!(w.exec.last().unwrap().output.contains(&format!(
            "salida {}",
            EXEC_MAX_ENTRIES + 49
        )));

        for _ in 0..TRACE_MAX_ENTRIES + 10 {
            w.trace_push(TraceEntry { phase: "think".into(), ..Default::default() });
        }
        assert_eq!(w.trace.len(), TRACE_MAX_ENTRIES);
    }

    #[test]
    fn relanzar_un_fork_reemplaza_su_fila() {
        let mut w = Workspace::default();
        w.fork_start("scan-1", "escanea", "opus");
        w.fork_start("scan-1", "escanea otra vez", "opus");
        assert_eq!(w.forks.len(), 1, "un reintento no duplica la fila");
        assert_eq!(w.forks[0].instruction, "escanea otra vez");
        assert_eq!(w.forks_running(), 1);
    }

    #[test]
    fn recogido_es_distinto_de_terminado() {
        // La distinción es toda la razón de ser del estado: entre que un fork
        // acaba y que `wait_task` lo recoge, Lucy sigue trabajando y el operador
        // tiene que poder verlo.
        let mut w = Workspace::default();
        w.fork_start("scan-1", "escanea", "opus");
        w.fork_finish("scan-1", ForkStatus::Done, "3 hallazgos");
        assert_eq!(w.forks[0].status, ForkStatus::Done);
        assert_eq!(w.forks_running(), 0);
        assert!(w.forks[0].ms.is_some(), "la duración sale de la hora guardada");

        w.fork_collected("scan-1");
        assert_eq!(w.forks[0].status, ForkStatus::Collected);
        assert_eq!(w.forks[0].result, "3 hallazgos", "recoger no borra el resultado");
    }

    #[test]
    fn reset_limpia_los_carriles_pero_no_los_ids() {
        let mut w = Workspace::default();
        w.exec_push(exec("uno"));
        let antes = w.exec[0].id.clone();
        w.reset();
        assert!(w.is_empty());
        w.exec_push(exec("dos"));
        assert_ne!(
            w.exec[0].id, antes,
            "dos entradas de dos tareas distintas no pueden compartir id"
        );
    }

    #[test]
    fn actualizar_un_paso_que_no_existe_se_nota() {
        // Un `false` aquí significa que el bucle y el panel hablan de pasos
        // distintos — que es un fallo real, no un caso normal que ignorar.
        let mut w = Workspace::default();
        let id = w.plan_append(PlanStep { label: "leer disco".into(), ..Default::default() });
        assert!(w.plan_update(&id, StepStatus::Done, Some(120)));
        assert_eq!(w.plan[0].status, StepStatus::Done);
        assert_eq!(w.plan[0].ms, Some(120));
        assert!(!w.plan_update("no-existe", StepStatus::Done, None));
    }
}
