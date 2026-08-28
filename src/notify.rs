//! El canal por el que Lucy puede decirte algo cuando no la estás mirando.
//!
//! LA MITAD QUE FALTABA PARA QUE VIGILAR SIRVIERA DE ALGO. La cara nueva no
//! tiene ni toast, ni bandeja, ni nada: todo lo que Lucy sabe hay que ir a
//! buscarlo abriendo una pestaña. La V1 sí tenía —`tauri-plugin-notification`—
//! y al cambiar de cara se quedó atrás. Un vigilante por muy listo que sea no
//! sirve de nada sin por dónde hablar, así que esto va ANTES que el vigilante.
//!
//! ── EL TOAST ES LA ENTREGA, NO EL REGISTRO ──────────────────────────────────
//!
//! Es la decisión que ordena todo lo demás. Un aviso se ANOTA EN DISCO primero y
//! se intenta enseñar después, y en ese orden exacto:
//!
//! ```text
//!   1. se escribe la fila           <- pase lo que pase, el operador puede enterarse
//!   2. se intenta el toast          <- puede fallar por seis motivos distintos
//!   3. se anota cómo fue la entrega <- para poder mirar si el canal va bien
//! ```
//!
//! Porque un toast se pierde por motivos que no son culpa de nadie: el Asistente
//! de concentración está puesto, las notificaciones de la aplicación están
//! apagadas, la sesión está bloqueada, el equipo estaba suspendido, o el AUMID
//! todavía no está registrado porque se está corriendo desde `cargo run` y no
//! desde la instalación. Si el toast fuera el único registro, «Lucy no avisó» y
//! «Lucy avisó y Windows se lo tragó» serían indistinguibles — y la primera vez
//! que pasara lo segundo, el operador dejaría de fiarse del vigilante entero.
//!
//! Con la fila puesta, un aviso que no salió en pantalla sigue estando, con su
//! hora y con el motivo por el que no se vio.
//!
//! ── POR QUÉ POWERSHELL Y NO UN CRATE DE WINRT ───────────────────────────────
//!
//! Por lo mismo que `elevate` lanza el UAC con PowerShell en vez de traerse una
//! biblioteca de COM, y que `health` mide por CIM: la casa ya tiene un canal al
//! sistema que está probado, y meter enlaces de WinRT en un crate que compila
//! también fuera de Windows cuesta más de lo que ahorra. Son doscientos
//! milisegundos por aviso, y esto se dispara unas cuantas veces al día.
//!
//! El script viaja en base64 UTF-16LE dentro de `-EncodedCommand`, igual que el
//! de la elevación y el de WinRM, para que ni un nombre de servicio con comillas
//! pueda cerrar lo que lo rodea.
//!
//! ── EL CANAL ENTREGA, NO DECIDE ─────────────────────────────────────────────
//!
//! Aquí NO se decide si un aviso merece salir. No hay silencio nocturno, ni
//! antirrepetición, ni prioridades. Eso es de la capa de arriba, y mezclarlo
//! aquí haría imposible probar las dos cosas por separado — además de que un
//! canal que se calla cosas por su cuenta es un canal en el que no se confía.
//!
//! Lo único que este módulo aporta a esa decisión es memoria: guarda la `clave`
//! de cada aviso y `ultimo_de` dice cuándo se mandó por última vez uno con esa
//! clave. Con eso, quien decide puede decidir.

use crate::thresholds::Nivel;

/// La identidad con la que Windows atribuye el toast.
///
/// TIENE QUE COINCIDIR CON LA DEL INSTALADOR, letra por letra. Windows no
/// enseña un toast de un AUMID que no conozca, y solo lo conoce si hay un
/// acceso directo en el menú de inicio que lo declare — por eso el `.wxs` y el
/// `.nsi` le ponen `System.AppUserModel.ID` al suyo con esta misma cadena. Si
/// las dos dejan de coincidir, el toast deja de salir y no hay ningún error que
/// lo explique: simplemente no aparece.
///
/// Un test comprueba que el instalador la sigue llevando.
pub const AUMID: &str = "IvanEduardoLuna.Lucy";

/// Lo que se quiere decir.
#[derive(Debug, Clone, PartialEq)]
pub struct Aviso {
    pub titulo: String,
    pub cuerpo: String,
    /// Reutiliza la escala de `thresholds` A PROPÓSITO. Ese módulo existe porque
    /// había tres escalas distintas para el mismo dato en la misma pantalla;
    /// inventar aquí una cuarta para los avisos sería repetir el error con más
    /// convicción.
    pub nivel: Nivel,
    /// De qué equipo habla. Vacío = éste.
    pub equipo: String,
    /// De qué va, para que quien decide pueda saber si ya lo dijo. No se enseña.
    ///
    /// Algo estable y estrecho: `disco:C:` y no `disco al 94 %`, porque la
    /// segunda cambia con cada medida y entonces todo aviso es nuevo.
    pub clave: String,
}

impl Aviso {
    pub fn nuevo(titulo: impl Into<String>, cuerpo: impl Into<String>) -> Self {
        Self {
            titulo: titulo.into(),
            cuerpo: cuerpo.into(),
            nivel: Nivel::Aviso,
            equipo: String::new(),
            clave: String::new(),
        }
    }

    pub fn con_nivel(mut self, n: Nivel) -> Self {
        self.nivel = n;
        self
    }

    pub fn en_equipo(mut self, e: &str) -> Self {
        self.equipo = e.to_string();
        self
    }

    pub fn con_clave(mut self, c: &str) -> Self {
        self.clave = c.to_string();
        self
    }
}

/// Cómo acabó el intento de enseñarlo.
///
/// NO HAY UN «ENSEÑADO» Y ES A PROPÓSITO. Windows no dice si un toast se
/// entregó, y lo he comprobado de las tres maneras que hay:
///
/// ```text
///   CreateToastNotifier con un AUMID que no existe   NO lanza
///   .Setting sobre ese mismo notificador             dice «Enabled»
///   .Show(toast)                                     vuelve sin error
/// ```
///
/// …y el aviso no llega a ninguna parte. Un `Enseñado` en este enum sería una
/// afirmación que nadie ha comprobado, y en un canal de avisos esa mentira es
/// cara: el operador dejaría de mirar el registro creyendo que ya se le avisó.
///
/// Lo único que se puede afirmar es que la llamada se hizo. Para saber si de
/// verdad llega, `diagnostico` mira el centro de notificaciones de Windows —que
/// es la única fuente que no miente— y esa comprobación es demasiado cara para
/// hacerla en cada aviso.
#[derive(Debug, Clone, PartialEq)]
pub enum Entrega {
    /// El sistema aceptó la llamada sin quejarse. NO garantiza que se viera.
    Intentado,
    /// Ni siquiera se pudo intentar, y por qué. El aviso sigue en el registro.
    NoSePudo(String),
}

impl Entrega {
    pub fn se_intento(&self) -> bool {
        matches!(self, Entrega::Intentado)
    }
}

/// Una fila del registro de avisos.
#[derive(Debug, Clone, PartialEq)]
pub struct Registrado {
    pub id: i64,
    pub ts: i64,
    pub titulo: String,
    pub cuerpo: String,
    pub nivel: Nivel,
    pub equipo: String,
    pub clave: String,
    /// `true` si el toast salió. `false` = está solo en el registro.
    pub enseñado: bool,
    /// Por qué no se pudo enseñar. Vacío si salió.
    pub motivo: String,
    pub visto: bool,
}

pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS avisos (
                 id       INTEGER PRIMARY KEY AUTOINCREMENT,
                 ts       INTEGER NOT NULL,
                 titulo   TEXT    NOT NULL,
                 cuerpo   TEXT    NOT NULL DEFAULT '',
                 nivel    INTEGER NOT NULL DEFAULT 1,
                 equipo   TEXT    NOT NULL DEFAULT '',
                 clave    TEXT    NOT NULL DEFAULT '',
                 enseñado INTEGER NOT NULL DEFAULT 0,
                 motivo   TEXT    NOT NULL DEFAULT '',
                 visto    INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_avisos_ts ON avisos(ts DESC);
             CREATE INDEX IF NOT EXISTS idx_avisos_clave ON avisos(clave, ts DESC);
             CREATE INDEX IF NOT EXISTS idx_avisos_sin_ver ON avisos(visto, ts DESC);",
        )
        .map_err(|e| format!("notify: esquema: {e}"))
    })
}

fn ahora() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn a_num(n: Nivel) -> i64 {
    match n {
        Nivel::Ok => 0,
        Nivel::Aviso => 1,
        Nivel::Critico => 2,
    }
}

fn de_num(n: i64) -> Nivel {
    match n {
        0 => Nivel::Ok,
        2 => Nivel::Critico,
        _ => Nivel::Aviso,
    }
}

/// Anota el aviso y trata de enseñarlo. BLOQUEANTE: quien llama ya está en un
/// hilo, porque el toast lanza un PowerShell.
///
/// EL ORDEN IMPORTA Y ES EL DE LA CABECERA. Primero la fila, después la
/// pantalla. Si esto se invirtiera, un fallo del toast se llevaría por delante
/// el aviso entero y nadie se enteraría nunca de que hubo algo que contar.
pub fn envia(a: &Aviso) -> Entrega {
    // Si ni siquiera se puede escribir la fila, se sigue intentando el toast: un
    // aviso en pantalla sin registro es peor que con él, pero muchísimo mejor
    // que ningún aviso. De ahí el `.ok()` en vez de un `?`.
    let id = anota(a).ok();
    let entrega = enseña(a);
    if let Some(id) = id {
        let _ = marca_entrega(id, &entrega);
    }
    entrega
}

fn anota(a: &Aviso) -> Result<i64, String> {
    ensure_schema()?;
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO avisos (ts, titulo, cuerpo, nivel, equipo, clave)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                ahora(),
                a.titulo,
                a.cuerpo,
                a_num(a.nivel),
                a.equipo,
                a.clave
            ],
        )
        .map_err(|e| format!("notify: anotar: {e}"))?;
        Ok(c.last_insert_rowid())
    })
}

fn marca_entrega(id: i64, e: &Entrega) -> Result<(), String> {
    let (ok, motivo) = match e {
        Entrega::Intentado => (1_i64, String::new()),
        Entrega::NoSePudo(m) => (0, m.clone()),
    };
    crate::with_db(|c| {
        c.execute(
            "UPDATE avisos SET enseñado = ?2, motivo = ?3 WHERE id = ?1",
            rusqlite::params![id, ok, motivo],
        )
        .map_err(|e| format!("notify: entrega: {e}"))?;
        Ok(())
    })
}

/// Los avisos que el operador no ha marcado como vistos, el más reciente
/// primero.
pub fn sin_ver(limite: usize) -> Vec<Registrado> {
    if ensure_schema().is_err() {
        return Vec::new();
    }
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT id, ts, titulo, cuerpo, nivel, equipo, clave, enseñado, motivo, visto
                 FROM avisos WHERE visto = 0 ORDER BY ts DESC, id DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let v = st
            .query_map([limite.clamp(1, 500) as i64], |r| {
                Ok(Registrado {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    titulo: r.get(2)?,
                    cuerpo: r.get(3)?,
                    nivel: de_num(r.get(4)?),
                    equipo: r.get(5)?,
                    clave: r.get(6)?,
                    enseñado: r.get::<_, i64>(7)? != 0,
                    motivo: r.get(8)?,
                    visto: r.get::<_, i64>(9)? != 0,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|x| x.ok())
            .collect();
        Ok(v)
    })
    .unwrap_or_default()
}

/// Cuántos hay sin ver. Es lo que pinta la insignia.
pub fn cuantos_sin_ver() -> usize {
    if ensure_schema().is_err() {
        return 0;
    }
    crate::with_db(|c| {
        c.query_row("SELECT COUNT(*) FROM avisos WHERE visto = 0", [], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())
    })
    .map(|n| n as usize)
    .unwrap_or(0)
}

/// Marca uno como visto. `None` = todos.
pub fn marca_visto(id: Option<i64>) -> Result<(), String> {
    ensure_schema()?;
    crate::with_db(|c| {
        match id {
            Some(i) => c.execute("UPDATE avisos SET visto = 1 WHERE id = ?1", [i]),
            None => c.execute("UPDATE avisos SET visto = 1 WHERE visto = 0", []),
        }
        .map_err(|e| format!("notify: marcar visto: {e}"))?;
        Ok(())
    })
}

/// Cuándo se mandó por última vez un aviso con esta clave. `None` = nunca.
///
/// LO ÚNICO QUE ESTE MÓDULO APORTA A LA DECISIÓN de si algo merece decirse. No
/// decide él: da el dato para que decida quien tiene que hacerlo. Un vigilante
/// que muestrea cada cinco minutos necesita saber que el disco lleno ya se dijo
/// hace un rato, y esa consulta no tiene por qué reinventarla cada capa.
pub fn ultimo_de(clave: &str) -> Option<i64> {
    if clave.is_empty() || ensure_schema().is_err() {
        return None;
    }
    crate::with_db(|c| {
        c.query_row(
            "SELECT MAX(ts) FROM avisos WHERE clave = ?1",
            [clave],
            |r| r.get::<_, Option<i64>>(0),
        )
        .map_err(|e| e.to_string())
    })
    .ok()
    .flatten()
}

/// Borra los avisos vistos de hace más de `dias`.
///
/// Solo los VISTOS: uno sin ver es algo que el operador todavía no sabe, y
/// borrarlo por antiguo sería decidir por él que ya no importa.
pub fn prune(dias: i64) -> Result<usize, String> {
    if dias <= 0 {
        return Ok(0);
    }
    ensure_schema()?;
    crate::with_db(|c| {
        c.execute(
            "DELETE FROM avisos WHERE visto = 1 AND ts < (strftime('%s','now') - ?1 * 86400)",
            [dias],
        )
        .map_err(|e| format!("notify: podar: {e}"))
    })
}

// ── La pantalla ─────────────────────────────────────────────────────────────

/// Escapa lo que va dentro del XML del toast.
///
/// NO ES COSMÉTICO. El cuerpo de un aviso lleva nombres de servicio, rutas y
/// mensajes de error que vienen de la máquina: un `&` en un nombre de servicio
/// —o un `<` en una ruta rara— rompe el XML entero y el toast no sale, sin
/// decir por qué. Y como el texto puede venir de la salida de un comando, dejar
/// pasar `<` es además dejar que algo de fuera escriba etiquetas dentro de la
/// notificación.
pub fn escapa_xml(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            // Los de control rompen el XML 1.0 y no aportan nada a una frase.
            c if (c as u32) < 0x20 && c != '\n' => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// El script que enseña el toast.
///
/// Separado de quien lo ejecuta para poder probarlo sin Windows delante: lo que
/// se puede equivocar aquí es el escapado y la forma del XML, y las dos se leen
/// en una cadena.
pub fn build_toast(a: &Aviso) -> String {
    // `ToastGeneric` con dos textos es la plantilla que entiende todo Windows 10
    // y 11 sin depender de que la aplicación tenga paquete. El `duration` largo
    // solo en lo crítico: un aviso que se queda clavado en pantalla para decir
    // que la CPU subió al 80 % enseña a cerrarlos sin leerlos.
    let dur = if a.nivel == Nivel::Critico { " duration=\"long\"" } else { "" };
    let equipo = if a.equipo.is_empty() {
        String::new()
    } else {
        format!("<text placement=\"attribution\">{}</text>", escapa_xml(&a.equipo))
    };
    let xml = format!(
        "<toast{dur}><visual><binding template=\"ToastGeneric\">\
         <text>{}</text><text>{}</text>{equipo}\
         </binding></visual></toast>",
        escapa_xml(&a.titulo),
        escapa_xml(&a.cuerpo),
    );
    // UN SOLO INTENTO, y aquí hubo una identidad prestada que quité.
    //
    // La idea era: si el AUMID de Lucy no está registrado, reintentar con el de
    // Windows PowerShell —que existe en toda máquina— para que el aviso saliera
    // igual aunque con el nombre de otro. Se midió y NO ENTREGA TAMPOCO: el
    // toast con la identidad prestada no llega al centro de notificaciones, y
    // como `Show()` no se queja, el reintento solo servía para dar la impresión
    // de tener red de seguridad. Una red que no sujeta es peor que ninguna.
    //
    // Lo que sí queda es `diagnostico`, que va a mirar si el aviso llegó de
    // verdad en vez de suponerlo.
    format!(
        "$ErrorActionPreference = 'Stop'
         [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
         [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
         $x = New-Object Windows.Data.Xml.Dom.XmlDocument
         $x.LoadXml('{xml}')
         $t = New-Object Windows.UI.Notifications.ToastNotification $x
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{AUMID}').Show($t)
"
    )
}


#[cfg(windows)]
fn enseña(a: &Aviso) -> Entrega {
    match crate::shell::run_powershell_utf8(&build_toast(a)) {
        Ok((_, _, true)) => Entrega::Intentado,
        Ok((_, err, false)) => Entrega::NoSePudo(recorta(&err)),
        Err(e) => Entrega::NoSePudo(recorta(&e)),
    }
}

/// Fuera de Windows no hay toast, y decirlo es mejor que fingirlo: el aviso
/// queda anotado igual y `sin_ver` lo devuelve.
#[cfg(not(windows))]
fn enseña(_a: &Aviso) -> Entrega {
    Entrega::NoSePudo("este sistema no tiene notificaciones de Windows".into())
}

// ── ¿De verdad llega? ───────────────────────────────────────────────────────

/// Lo que se pudo averiguar sobre si el canal entrega.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostico {
    /// La única respuesta que vale: el aviso de prueba apareció en el centro de
    /// notificaciones de Windows.
    pub entrega: bool,
    /// Qué se vio, en una línea que se pueda enseñar.
    pub detalle: String,
}

/// Manda un aviso de prueba y comprueba EN EL CENTRO DE NOTIFICACIONES si
/// apareció.
///
/// LA ÚNICA COMPROBACIÓN QUE NO MIENTE, y existe porque las tres que ofrece la
/// API sí lo hacen: con un AUMID que no está registrado, `CreateToastNotifier`
/// no lanza, su `.Setting` dice «Enabled» y `.Show()` vuelve limpio — y el aviso
/// no llega a ninguna parte. Medido en esta máquina, con tres variantes: la
/// clave `HKCU\Software\Classes\AppUserModelId` sola, un acceso directo del
/// menú de inicio sin la propiedad, y la identidad prestada de PowerShell.
/// Ninguna entregó, y ninguna dio el más mínimo error.
///
/// Windows guarda lo que sí entrega en una base SQLite suya
/// (`wpndatabase.db`), así que la pregunta se contesta mirando ahí. Es cara
/// —copia un fichero de un mega y espera a que el sistema escriba— y por eso no
/// se hace en cada aviso: esto es un botón de «comprueba el canal», no parte
/// del camino normal.
///
/// UNA RESPUESTA NEGATIVA NO SIGNIFICA QUE EL AVISO SE PIERDA. El registro de
/// `avisos` lo tiene igual; lo que se pierde es el aviso EN PANTALLA, que es
/// justo lo que esto sirve para saber.
#[cfg(windows)]
pub fn diagnostico() -> Diagnostico {
    const MARCA: &str = "Lucy · comprobación del canal";
    let a = Aviso::nuevo(MARCA, "Si ves esto, los avisos de Lucy llegan a la pantalla.")
        .con_clave("diagnostico");
    if let Entrega::NoSePudo(e) = enseña(&a) {
        return Diagnostico { entrega: false, detalle: format!("no se pudo lanzar: {e}") };
    }
    // El sistema escribe la fila de forma asíncrona. Sin esta espera, un canal
    // que SÍ funciona se diagnosticaría como roto la mitad de las veces.
    std::thread::sleep(std::time::Duration::from_millis(2_500));
    match busca_en_el_centro(MARCA) {
        Ok(true) => Diagnostico {
            entrega: true,
            detalle: "el aviso de prueba apareció en el centro de notificaciones".into(),
        },
        Ok(false) => Diagnostico {
            entrega: false,
            detalle: "Windows aceptó la llamada sin quejarse y el aviso no llegó al centro de \
                      notificaciones. Es lo que pasa cuando el AUMID no está registrado por un \
                      acceso directo del menú de inicio que lo declare."
                .into(),
        },
        Err(e) => Diagnostico { entrega: false, detalle: format!("no se pudo comprobar: {e}") },
    }
}

#[cfg(not(windows))]
pub fn diagnostico() -> Diagnostico {
    Diagnostico { entrega: false, detalle: "este sistema no tiene notificaciones de Windows".into() }
}

/// Busca el texto en la base de notificaciones de Windows.
///
/// SE COPIA ANTES DE ABRIR porque el sistema la tiene abierta: leerla en su
/// sitio da «database is locked» la mayoría de las veces, y un diagnóstico que
/// falla la mitad de las veces no diagnostica nada.
#[cfg(windows)]
fn busca_en_el_centro(texto: &str) -> Result<bool, String> {
    let base = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").map_err(|_| "sin LOCALAPPDATA")?)
        .join("Microsoft\\Windows\\Notifications\\wpndatabase.db");
    if !base.exists() {
        return Err("este Windows no tiene centro de notificaciones".into());
    }
    let copia = std::env::temp_dir().join("lucy-wpn-diagnostico.db");
    std::fs::copy(&base, &copia).map_err(|e| format!("no se pudo copiar: {e}"))?;
    let c = rusqlite::Connection::open_with_flags(
        &copia,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("no se pudo abrir: {e}"))?;
    // Los diez últimos toast: si el nuestro se mandó hace tres segundos y no
    // está entre ellos, no llegó.
    let mut st = c
        .prepare("SELECT Payload FROM Notification WHERE Type = 'toast' ORDER BY ArrivalTime DESC LIMIT 10")
        .map_err(|e| format!("consulta: {e}"))?;
    let hay = st
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .map_err(|e| format!("consulta: {e}"))?
        .filter_map(|x| x.ok())
        .any(|p| String::from_utf8_lossy(&p).contains(texto));
    let _ = std::fs::remove_file(&copia);
    Ok(hay)
}

fn recorta(s: &str) -> String {
    let limpio: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    limpio.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_escapado_no_deja_romper_el_xml() {
        // Un nombre de servicio con `&` es de lo más normal —«Firewall & NAT»—
        // y sin escapar deja el XML inválido: el toast no sale y no hay error
        // que lo explique.
        assert_eq!(escapa_xml("Firewall & NAT"), "Firewall &amp; NAT");
        assert_eq!(escapa_xml("<text>fuera</text>"), "&lt;text&gt;fuera&lt;/text&gt;");
        assert_eq!(escapa_xml("ruta \"C:\\a\""), "ruta &quot;C:\\a&quot;");
        // Los de control se van: rompen el XML 1.0 y no aportan a una frase.
        assert_eq!(escapa_xml("a\u{1}b"), "a b");
        assert_eq!(escapa_xml("linea\nlinea"), "linea\nlinea", "el salto sí se conserva");
    }

    #[test]
    fn el_texto_de_la_maquina_no_puede_escribir_etiquetas_dentro() {
        // El cuerpo puede venir de la salida de un comando. Sin escapar, eso es
        // dejar que algo de fuera escriba dentro de la notificación.
        let a = Aviso::nuevo("x", "</text><text>inyectado</text>");
        let s = build_toast(&a);
        assert!(!s.contains("<text>inyectado"), "se coló una etiqueta desde el cuerpo");
        assert!(s.contains("&lt;/text&gt;"));
    }

    #[test]
    fn no_hay_identidad_prestada_porque_no_entregaba() {
        // Aquí hubo un reintento con el AUMID de Windows PowerShell para cuando
        // el de Lucy no estuviera registrado. Se midió contra el centro de
        // notificaciones y NO ENTREGA: solo servía para aparentar una red de
        // seguridad. Este test existe para que no vuelva sin haberla medido.
        let s = build_toast(&Aviso::nuevo("t", "c"));
        assert!(!s.contains("WindowsPowerShell"), "volvió la identidad prestada sin medirla");
        assert_eq!(s.matches("CreateToastNotifier").count(), 1, "hay más de un intento");
    }

    #[test]
    fn el_script_lleva_el_mismo_aumid_que_la_constante() {
        // Si el script y la constante derivaran, el toast dejaría de salir sin
        // ningún error: Windows simplemente no enseña lo de un AUMID que no
        // conoce.
        let s = build_toast(&Aviso::nuevo("t", "c"));
        assert!(s.contains(AUMID), "el script no usa el AUMID declarado");
        assert!(s.contains("CreateToastNotifier"));
    }

    #[test]
    fn solo_lo_critico_se_queda_clavado_en_pantalla() {
        // Un aviso que no se va solo, para decir que la CPU subió al 80 %,
        // enseña a cerrarlos sin leerlos — y entonces el canal está muerto.
        let normal = build_toast(&Aviso::nuevo("t", "c").con_nivel(Nivel::Aviso));
        let grave = build_toast(&Aviso::nuevo("t", "c").con_nivel(Nivel::Critico));
        assert!(!normal.contains("duration"), "un aviso normal se queda fijo");
        assert!(grave.contains("duration=\"long\""));
    }

    #[test]
    fn el_equipo_sale_como_atribucion_y_solo_si_lo_hay() {
        let local = build_toast(&Aviso::nuevo("t", "c"));
        assert!(!local.contains("attribution"), "un aviso de este equipo lleva atribución vacía");
        let remoto = build_toast(&Aviso::nuevo("t", "c").en_equipo("SRV-04"));
        assert!(remoto.contains("attribution"));
        assert!(remoto.contains("SRV-04"));
    }

    #[test]
    fn los_niveles_van_y_vuelven_de_la_base() {
        for n in [Nivel::Ok, Nivel::Aviso, Nivel::Critico] {
            assert_eq!(de_num(a_num(n)), n);
        }
        // Un valor que no reconoce no revienta ni se inventa una alarma: cae en
        // el nivel de en medio, que es el que menos afirma.
        assert_eq!(de_num(99), Nivel::Aviso);
    }
}
