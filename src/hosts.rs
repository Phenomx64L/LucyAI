//! El registro de equipos remotos.
//!
//! ES EL MISMO ÍNDICE QUE LA APP, byte a byte. Vive en el Credential Manager de
//! Windows bajo el servicio `LucySysAdmin`, en la entrada
//! `LucyHost_lucy_hosts_index`, como un array JSON; la contraseña de cada equipo
//! va aparte, en `LucyHost_{id}`. Ese detalle es lo que permite que las dos
//! interfaces vean exactamente los mismos equipos sin sincronizar nada — si el
//! índice viviera en el `localStorage` del WebView, el shell nativo no podría
//! verlo y habría que dar de alta cada máquina dos veces.
//!
//! POR ESO LOS NOMBRES DE CAMPO SON LOS DEL JSON de la app y no los que
//! elegiría uno en Rust: `type` para el sistema operativo, `sshKeyPath` en
//! camelCase. Cambiar uno rompe la compatibilidad en silencio — el otro lado
//! deja de encontrar el dato y usa su valor por defecto, que casi siempre
//! parece razonable.
//!
//! ESCRIBE, al contrario que la versión anterior de este módulo. Era de solo
//! lectura y decía que dar de alta un equipo «sigue siendo cosa de la vista de
//! Configuración» — de la de la app Tauri, es decir: para añadir una máquina
//! había que abrir el WebView del que este shell existe para escapar.

use serde::{Deserialize, Serialize};

/// El servicio bajo el que la app guarda todo en el Credential Manager.
const SERVICE: &str = "LucySysAdmin";
/// Prefijo de cada entrada. Lo pone `save_host_credential` en la app.
const PREFIX: &str = "LucyHost_";
/// La entrada con el índice completo.
const INDEX: &str = "lucy_hosts_index";

/// Cómo se llega a un equipo.
///
/// La tabla trae el puerto por defecto de cada uno, que es la mitad del valor:
/// nadie recuerda que WinRM es 5985 y RDP 3389, y equivocarse produce un fallo
/// de red que no se parece en nada a «te has equivocado de puerto».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Winrm,
    Ssh,
    Rdp,
    Snmp,
    Docker,
    K8s,
    Postgres,
    Mysql,
    Mongodb,
    Redis,
    Mssql,
}

impl Protocol {
    pub const ALL: [Self; 11] = [
        Self::Winrm,
        Self::Ssh,
        Self::Rdp,
        Self::Snmp,
        Self::Docker,
        Self::K8s,
        Self::Postgres,
        Self::Mysql,
        Self::Mongodb,
        Self::Redis,
        Self::Mssql,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Winrm => "winrm",
            Self::Ssh => "ssh",
            Self::Rdp => "rdp",
            Self::Snmp => "snmp",
            Self::Docker => "docker",
            Self::K8s => "k8s",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
            Self::Mongodb => "mongodb",
            Self::Redis => "redis",
            Self::Mssql => "mssql",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Winrm => "Windows (WinRM)",
            Self::Ssh => "Linux (SSH)",
            Self::Rdp => "Windows (RDP)",
            Self::Snmp => "SNMP (Red)",
            Self::Docker => "Docker API (TCP)",
            Self::K8s => "Kubernetes API",
            Self::Postgres => "PostgreSQL",
            Self::Mysql => "MySQL / MariaDB",
            Self::Mongodb => "MongoDB",
            Self::Redis => "Redis",
            Self::Mssql => "SQL Server (MSSQL)",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Winrm => 5985,
            Self::Ssh => 22,
            Self::Rdp => 3389,
            Self::Snmp => 161,
            Self::Docker => 2375,
            Self::K8s => 6443,
            Self::Postgres => 5432,
            Self::Mysql => 3306,
            Self::Mongodb => 27017,
            Self::Redis => 6379,
            Self::Mssql => 1433,
        }
    }

    /// `windows` o `linux`, que es lo que se guarda en el campo `type`.
    pub fn os(self) -> &'static str {
        match self {
            Self::Winrm | Self::Rdp | Self::Mssql => "windows",
            _ => "linux",
        }
    }

    /// La categoría que le corresponde. Se deduce del protocolo en vez de
    /// pedirla dos veces: elegir «PostgreSQL» y luego tener que decir que es una
    /// base de datos es un formulario que no se fía de lo que acabas de poner.
    pub fn category(self) -> Category {
        match self {
            Self::Postgres | Self::Mysql | Self::Mongodb | Self::Redis | Self::Mssql => {
                Category::Database
            }
            Self::Docker => Category::Container,
            Self::K8s => Category::Kubernetes,
            Self::Snmp => Category::Network,
            _ => Category::Shell,
        }
    }

    /// El grupo del desplegable. Once protocolos en una lista plana no se leen.
    pub fn group(self) -> &'static str {
        match self.category() {
            Category::Shell => "Shell / Acceso remoto",
            Category::Database => "Bases de datos",
            _ => "Infraestructura",
        }
    }

    /// Si este shell sabe ejecutar comandos contra él.
    ///
    /// Solo WinRM y SSH. Los demás están en la lista porque el operador quiere
    /// tenerlos apuntados —un Redis o un Kubernetes son parte del parque— pero
    /// abrirles una terminal no significa nada, y ofrecerlo sería prometer algo
    /// que no se cumple.
    pub fn can_shell(self) -> bool {
        matches!(self, Self::Winrm | Self::Ssh)
    }

    /// Lo que hay que hacer en el equipo remoto para que esto funcione, cuando
    /// hay que hacer algo. Vacío = nada.
    pub fn requirement(self) -> &'static str {
        match self {
            Self::Winrm => "El servidor remoto debe tener WinRM habilitado. Ejecuta allí: \
                            Enable-PSRemoting -Force",
            Self::Ssh => "Por CLAVE, no por contraseña: la confianza se establece antes. \
                          Autoriza tu clave pública en el servidor (`~/.ssh/authorized_keys`) \
                          o ten la privada cargada en `ssh-agent`.",
            _ => "",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == k)
    }
}

/// Para qué sirve el equipo. Se guarda porque la app la guarda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Shell,
    Database,
    Container,
    Kubernetes,
    Network,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::Shell => "Servidor / Shell",
            Self::Database => "Base de datos",
            Self::Container => "Contenedor (Docker)",
            Self::Kubernetes => "Kubernetes",
            Self::Network => "Dispositivo de red",
        }
    }
}

/// Los ocho colores del selector, en el mismo orden que la app.
pub const COLORS: [&str; 8] = [
    "#10b981", "#ef4444", "#3b82f6", "#f59e0b", "#a78bfa", "#ec4899", "#22d3ee", "#f97316",
];

/// Un equipo del índice.
///
/// Los `skip_serializing_if` no son cosmética: la app omite `dbType` cuando la
/// categoría no es base de datos, y escribir un `null` donde ella no escribe
/// nada cambia el JSON que la otra mitad va a leer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub id: String,
    pub name: String,
    /// `windows` o `linux`. Se llama `type` en el JSON.
    #[serde(rename = "type", default = "os_windows")]
    pub os: String,
    #[serde(default = "proto_winrm")]
    pub protocol: Protocol,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub port: u16,
    #[serde(rename = "sshKeyPath", default, skip_serializing_if = "String::is_empty")]
    pub ssh_key_path: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "color_default")]
    pub color: String,
    #[serde(default = "cat_shell")]
    pub category: Category,
    #[serde(rename = "dbType", default, skip_serializing_if = "Option::is_none")]
    pub db_type: Option<String>,
}

fn os_windows() -> String {
    "windows".into()
}
fn proto_winrm() -> Protocol {
    Protocol::Winrm
}
fn color_default() -> String {
    COLORS[0].into()
}
fn cat_shell() -> Category {
    Category::Shell
}

impl Host {
    /// Un equipo nuevo, con los valores que corresponden a su protocolo.
    ///
    /// El id lleva la marca de tiempo, igual que en la app (`h_{millis}`): es lo
    /// que hace que los dos lados generen ids con la misma forma y ninguno se
    /// sorprenda al leer los del otro.
    pub fn nuevo(protocol: Protocol, millis: u64) -> Self {
        Self {
            id: format!("h_{millis}"),
            name: String::new(),
            os: protocol.os().into(),
            protocol,
            host: String::new(),
            username: String::new(),
            port: protocol.default_port(),
            ssh_key_path: String::new(),
            tags: Vec::new(),
            color: COLORS[0].into(),
            category: protocol.category(),
            db_type: None,
        }
    }

    /// Cambia el protocolo arrastrando lo que depende de él.
    ///
    /// El puerto se cambia SOLO si era el que tocaba: si el operador puso 5986
    /// —WinRM sobre HTTPS— y luego cambia de protocolo, pisarle el número sería
    /// borrarle una decisión deliberada.
    pub fn set_protocol(&mut self, p: Protocol) {
        if self.port == self.protocol.default_port() || self.port == 0 {
            self.port = p.default_port();
        }
        self.protocol = p;
        self.os = p.os().into();
        self.category = p.category();
        self.db_type = matches!(p.category(), Category::Database).then(|| p.key().to_string());
    }

    /// Qué le falta para poder guardarse. Vacío = está completo.
    ///
    /// Se devuelven TODOS los que faltan, no el primero: rellenar un campo,
    /// pulsar, y que te digan que falta otro es la forma más lenta de completar
    /// un formulario.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.name.trim().is_empty() {
            v.push("nombre");
        }
        if self.host.trim().is_empty() {
            v.push("dirección");
        }
        // Un SNMP o un Docker por TCP no tienen usuario, y exigirlo impediría
        // dar de alta la mitad del parque.
        if self.username.trim().is_empty() && self.protocol.can_shell() {
            v.push("usuario");
        }
        v
    }

    pub fn transport(&self) -> &'static str {
        self.protocol.label()
    }
}

// ── Persistencia ─────────────────────────────────────────────────────────────

fn entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &format!("{PREFIX}{key}"))
        .map_err(|e| format!("No se pudo abrir el almacén de credenciales: {e}"))
}

/// Lee el índice. Una lista vacía es un estado normal, no un error.
///
/// Un fallo del almacén tampoco se propaga: sin equipos remotos el resto de la
/// aplicación funciona igual, y romper una vista entera porque el Credential
/// Manager no contesta cambia una función que falta por una pantalla que no
/// arranca.
pub fn load() -> Vec<Host> {
    let Ok(e) = entry(INDEX) else { return Vec::new() };
    let Ok(raw) = e.get_password() else {
        return Vec::new();
    };
    parse(&raw)
}

/// Igual que `load`, separado para poder probarlo sin tocar el almacén.
pub fn parse(raw: &str) -> Vec<Host> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Guarda el índice entero.
pub fn save(hosts: &[Host]) -> Result<(), String> {
    let json = serde_json::to_string(hosts).map_err(|e| format!("serializando hosts: {e}"))?;
    entry(INDEX)?
        .set_password(&json)
        .map_err(|e| format!("guardando el índice de equipos: {e}"))
}

/// Guarda la contraseña de un equipo, aparte del índice.
///
/// APARTE Y NO DENTRO, como hace la app. El índice se lee entero para pintar una
/// lista; las contraseñas solo se tocan al conectar. Meterlas en el mismo blob
/// significaría tener todas las credenciales del parque en memoria cada vez que
/// se dibuja la barra lateral.
pub fn set_password(id: &str, password: &str) -> Result<(), String> {
    entry(id)?
        .set_password(password)
        .map_err(|e| format!("guardando la credencial: {e}"))
}

pub fn password(id: &str) -> Option<String> {
    entry(id).ok()?.get_password().ok()
}

/// Borra un equipo del índice y su credencial.
pub fn delete(hosts: &mut Vec<Host>, id: &str) -> Result<(), String> {
    hosts.retain(|h| h.id != id);
    // La credencial primero: si el índice se guarda y esto falla, queda una
    // contraseña huérfana en el almacén de la que ya nadie sabe el dueño.
    if let Ok(e) = entry(id) {
        let _ = e.delete_password();
    }
    save(hosts)
}

// ── Ejecución remota ─────────────────────────────────────────────────────────

/// Corre un script en un equipo remoto. Devuelve `(salida, error, ok)`.
///
/// LA CONTRASEÑA VA POR LA ENTRADA ESTÁNDAR, nunca en la línea de comandos ni
/// interpolada en el script. En los argumentos la vería cualquiera con un
/// `Get-Process`; interpolada en el literal de PowerShell, una contraseña que
/// contenga una comilla cierra la cadena y lo que venga detrás se ejecuta — que
/// es exactamente la inyección S1 del informe de seguridad de la app.
///
/// Y EL SCRIPT VA EN BASE64 UTF-16LE. No es por el transporte: es para que su
/// contenido no pueda cerrar el `ScriptBlock` que lo envuelve. Un script con una
/// llave suelta rompería el bloque y ejecutaría el resto fuera de él.
#[cfg(windows)]
pub fn run_remote(h: &Host, password: &str, script: &str) -> Result<(String, String, bool), String> {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    // La MISMA decisión que usa la versión con streaming. Dos copias de «qué
    // protocolo se puede y con qué credencial» acabarían discrepando, y la que
    // discrepara de menos dejaría pasar una ejecución que la otra rechaza.
    let (programa, args) = lanzamiento(h, script)?;

    let mut hijo = Command::new(&programa)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|e| format!("No se pudo lanzar PowerShell: {e}"))?;
    if let Some(mut s) = hijo.stdin.take() {
        let _ = writeln!(s, "{password}");
    }
    let out = hijo.wait_with_output().map_err(|e| format!("Fallo esperando a PowerShell: {e}"))?;
    Ok((
        crate::shell::decode_console(&out.stdout),
        crate::shell::decode_console(&out.stderr),
        out.status.success(),
    ))
}

#[cfg(not(windows))]
pub fn run_remote(
    _h: &Host,
    _password: &str,
    _script: &str,
) -> Result<(String, String, bool), String> {
    Err("la ejecución remota solo está implementada en Windows".into())
}

/// El script que envuelve al del operador para mandarlo por WinRM.
///
/// Port de `build_winrm_script`. Se comprueba por separado —hay un test— porque
/// es donde vive el escapado, y un escapado mal hecho aquí no da un error: da
/// una ejecución de más.
pub fn winrm_wrapper(h: &Host, script: &str) -> String {
    format!(
        // El try/catch NO es cortesía. Sin él, PowerShell formatea el error para
        // una consola: lo parte al ancho del búfer y le añade `+ CategoryInfo` y
        // `+ FullyQualifiedErrorId`. Lo que llega es un mensaje troceado del que
        // hay que reconstruir la frase. Con él llega `$_.Exception.Message`, que
        // es el texto tal cual lo escribió WinRM, entero y en una pieza — que es
        // justo lo que el operador necesita leer para saber qué arreglar.
        "$ErrorActionPreference = 'Stop'; \
         $pass_plain = [Console]::In.ReadLine(); \
         try {{ \
           $pass = ConvertTo-SecureString $pass_plain -AsPlainText -Force; \
           $cred = New-Object System.Management.Automation.PSCredential('{}', $pass); \
           Invoke-Command -ComputerName '{}' -Port {} -Credential $cred -ScriptBlock {{ \
             $s = [System.Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{}')); \
             Invoke-Expression $s \
           }} \
         }} catch {{ \
           [Console]::Error.WriteLine($_.Exception.Message); \
           exit 1 \
         }}",
        h.username.replace('\'', "''"),
        h.host.replace('\'', "''"),
        h.port,
        b64_utf16le(script),
    )
}

/// Lo mismo por SSH, con clave. La contraseña no cabe aquí — ver `run_remote`.
/// Los argumentos de `ssh` para este equipo, como argv.
///
/// COMO ARGV Y NO COMO UNA LÍNEA DE SHELL, que es lo que había. El camino de SSH
/// pasaba por PowerShell solo para que PowerShell llamara a `ssh`, y esa capa no
/// compraba nada: arriesgaba el entrecomillado —un nombre de usuario o una ruta
/// de clave con un carácter raro— y, sobre todo, SE COMÍA LA ENTRADA ESTÁNDAR.
/// Es justo la que hace falta para contestarle a un `sudo`.
///
/// El comando remoto va en base64 y se decodifica allí: así su contenido no
/// puede cerrar ninguna comilla, igual que en WinRM.
pub fn ssh_args(h: &Host, script: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if !h.ssh_key_path.is_empty() {
        v.push("-i".into());
        v.push(h.ssh_key_path.clone());
    }
    v.push("-o".into());
    // `accept-new` y no `no`: acepta una máquina nueva, pero sigue avisando si
    // la clave de una conocida CAMBIA — que es la señal que importa.
    v.push("StrictHostKeyChecking=accept-new".into());
    v.push("-p".into());
    v.push(h.port.to_string());
    v.push(format!("{}@{}", h.username, h.host));
    v.push(format!(
        "echo {} | base64 -d | sh",
        b64_utf8(script)
    ));
    v
}

fn b64_utf16le(s: &str) -> String {
    let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    crate::attach::b64_encode(&bytes)
}

fn b64_utf8(s: &str) -> String {
    crate::attach::b64_encode(s.as_bytes())
}

/// Una línea de una sesión remota, según de dónde venga.
///
/// El canal separa las tres porque el operador las lee distinto: lo que dijo el
/// comando, lo que dijo el error, y que ya terminó. Mezclarlas obliga a quien
/// pinta a adivinar, y adivinar mal convierte un aviso de PowerShell —que los
/// escribe por la salida de error aunque el comando funcione— en un fallo.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Out(String),
    Err(String),
    /// Terminó. `true` si el proceso salió bien.
    Done(bool),
}

/// Corre un script remoto ENTREGANDO LA SALIDA SEGÚN LLEGA.
///
/// POR QUÉ NO BASTABA `run_remote`. Esperaba a que el proceso terminara para
/// devolverlo todo junto, y un inventario sobre veinte servidores tarda minutos:
/// el operador veía «ejecutando…» sin una sola línea y sin forma de saber si
/// avanzaba o se había colgado. Un comando remoto largo es el caso normal, no el
/// raro.
///
/// `stop` se mira entre línea y línea, y además MATA EL PROCESO. Aquí no vale el
/// truco de «dejar de leer y ya se acabará solo» que se usa con un flujo HTTP:
/// al otro lado hay un `Invoke-Command` de verdad corriendo en una máquina de
/// verdad, y dejar de mirarlo no lo para.
#[cfg(windows)]
pub fn run_remote_streaming(
    h: &Host,
    password: &str,
    script: &str,
    tx: &std::sync::mpsc::Sender<Line>,
    stop: &std::sync::atomic::AtomicBool,
    entrada_tx: Option<&std::sync::mpsc::Sender<Option<std::process::ChildStdin>>>,
) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::atomic::Ordering;

    let (programa, args) = lanzamiento(h, script)?;
    let mut hijo = Command::new(&programa)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|e| format!("No se pudo lanzar `{programa}`: {e}"))?;

    // La entrada estándar SE CONSERVA para poder escribirle después. Antes se
    // soltaba aquí mismo —el `if let` la dejaba caer al salir— y con ella se iba
    // la única forma de contestarle a un `sudo` o a un «¿seguro? [y/N]». Es la
    // razón por la que la V2 declara que WinRM no admite entrada interactiva: en
    // WinRM sigue siendo verdad, porque la contraseña la consume el envoltorio;
    // en SSH ahora no.
    let mut entrada = hijo.stdin.take();
    if h.protocol == Protocol::Winrm {
        if let Some(s) = entrada.as_mut() {
            let _ = writeln!(s, "{password}");
        }
    }
    if let Some(tx_in) = entrada_tx {
        // Quien llama se queda con el extremo por el que escribir. Si no lo
        // quiere, la entrada se cierra al salir de aquí, que es lo correcto:
        // un programa remoto esperando una respuesta que nadie va a dar debe
        // recibir un fin de fichero y terminar, no colgarse para siempre.
        let _ = tx_in.send(entrada.take());
    }

    // El error se lee en OTRO hilo. Con un solo hilo, leer la salida hasta el
    // final antes de mirar el error se cuelga en cuanto el error llena su
    // tubería: el proceso se bloquea escribiendo y nunca cierra la salida que
    // estamos esperando. Es el bloqueo clásico de leer dos tuberías en serie.
    let err_tx = tx.clone();
    let err_pipe = hijo.stderr.take();
    let err_hilo = std::thread::spawn(move || {
        if let Some(e) = err_pipe {
            for l in BufReader::new(e).lines().map_while(Result::ok) {
                if l.trim().is_empty() {
                    continue;
                }
                if err_tx.send(Line::Err(crate::shell::decode_console(l.as_bytes()))).is_err() {
                    break;
                }
            }
        }
    });

    if let Some(o) = hijo.stdout.take() {
        for l in BufReader::new(o).split(b'\n') {
            if stop.load(Ordering::Relaxed) {
                let _ = hijo.kill();
                break;
            }
            let Ok(bytes) = l else { break };
            let texto = crate::shell::decode_console(&bytes);
            if tx.send(Line::Out(texto.trim_end_matches('\r').to_string())).is_err() {
                let _ = hijo.kill();
                break;
            }
        }
    }
    let ok = hijo.wait().map(|s| s.success()).unwrap_or(false);
    let _ = err_hilo.join();
    let _ = tx.send(Line::Done(ok && !stop.load(Ordering::Relaxed)));
    Ok(())
}

#[cfg(not(windows))]
pub fn run_remote_streaming(
    _h: &Host,
    _password: &str,
    _script: &str,
    tx: &std::sync::mpsc::Sender<Line>,
    _stop: &std::sync::atomic::AtomicBool,
    _entrada_tx: Option<&std::sync::mpsc::Sender<Option<std::process::ChildStdin>>>,
) -> Result<(), String> {
    let _ = tx.send(Line::Done(false));
    Err("la ejecución remota solo está implementada en Windows".into())
}

/// El constructor de envoltorio que corresponde a este equipo, o por qué no hay.
///
/// Se saca aparte porque lo comparten la versión que espera y la que va
/// entregando: dos copias de esta decisión acabarían discrepando en cuál es el
/// protocolo que sí se puede.
/// Con qué programa y qué argumentos se corre un script en este equipo.
///
/// Dos transportes de verdad y no uno disfrazado del otro: WinRM va envuelto en
/// PowerShell porque `Invoke-Command` ES de PowerShell; SSH se lanza directo,
/// porque meterlo dentro de un PowerShell solo servía para perder su entrada
/// estándar.
fn lanzamiento(h: &Host, script: &str) -> Result<(String, Vec<String>), String> {
    if !h.protocol.can_shell() {
        return Err(format!(
            "No se pueden ejecutar comandos contra {} por {}.",
            h.name,
            h.protocol.label()
        ));
    }
    match h.protocol {
        Protocol::Winrm => Ok((
            "powershell".into(),
            vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                crate::shell::ps_utf8(&winrm_wrapper(h, script)),
            ],
        )),
        // SSH VA SIEMPRE POR CLAVE, y eso es una decisión, no una carencia. El
        // modelo de operación es establecer la confianza entre las máquinas
        // antes —clave autorizada o `ssh-agent`— y entonces la contraseña
        // sobra: no hay secreto que guardar, ni que rotar, ni que teclear.
        //
        // Antes esto RECHAZABA si había una contraseña guardada, y era una
        // trampa: el formulario ofrecía el campo, rellenarlo era natural, y un
        // equipo bien configurado dejaba de funcionar por un dato que no debía
        // estar ahí. Ahora se ignora, y el formulario ya no lo pide.
        Protocol::Ssh => Ok(("ssh".into(), ssh_args(h, script))),
        _ => unreachable!("can_shell ya lo filtró"),
    }
}

/// El motivo de un fallo, reconstruido de lo que escribió PowerShell.
///
/// SE JUNTAN TODAS LAS LÍNEAS, y esto es lo que estaba mal. Antes se cogía la
/// primera —«la que dice qué pasó, el resto es rastro de pila»— y esa premisa es
/// falsa: PowerShell PARTE los mensajes largos al ancho de la consola. El de
/// WinRM ocupa cuatro o cinco renglones, así que el operador leía
/// «El cliente WinRM no puede» y ahí se acababa la explicación, justo antes de
/// la parte que dice qué hacer.
///
/// El rastro de pila sí se descarta, pero por lo que es —las líneas de
/// `+ CategoryInfo`, `+ FullyQualifiedErrorId` y las que empiezan por `at `— y
/// no por venir después.
pub fn motivo_de(err: &str, contesto: bool) -> String {
    let cuerpo: Vec<&str> = err
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take_while(|l| {
            !l.starts_with('+')
                && !l.starts_with("at ")
                && !l.starts_with("En línea")
                && !l.starts_with("At line")
        })
        .collect();
    if cuerpo.is_empty() {
        return if contesto {
            "el equipo contestó algo que no reconozco".into()
        } else {
            "sin respuesta del equipo".into()
        };
    }
    // Unidas por un espacio: eran un párrafo antes de que la consola las
    // partiera, y volver a juntarlas es lo que deja leerlo como una frase.
    truncar(&cuerpo.join(" "), 600)
}

/// Recorta un texto por caracteres, con puntos suspensivos.
///
/// Lo que llega de un equipo remoto no tiene tope: un rastro de pila de WinRM
/// son varias pantallas, y una etiqueta de interfaz no puede crecer con él.
fn truncar(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect::<String>() + "…"
}

/// Lo que se sabe de un equipo después de llamar a su puerta.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    /// El sistema que dijo tener. Vacío si contestó pero no supo decirlo.
    pub os: String,
    /// Lo que tardó. Distingue «funciona» de «funciona y va lento».
    pub ms: u64,
}

/// Llama a la puerta del equipo: ¿se llega, valen las credenciales, y qué corre?
///
/// LAS TRES COSAS EN UN VIAJE. Antes eran dos funciones y dos conexiones —una
/// para probar y otra para el sistema— y cada una paga su autenticación de
/// WinRM, que es la parte lenta. La V2 lo hace en una sola sonda y tiene razón.
///
/// La marca `Lucy:OK` no es adorno: prueba que lo que volvió es NUESTRO comando
/// y no un banner de bienvenida del servidor, un aviso de perfil o cualquier
/// otra cosa que un shell remoto escupe antes del prompt. Sin ella, un servidor
/// hablador pasa por conectado diciendo cualquier cosa.
pub fn probe(h: &Host, password: &str) -> Result<Probe, String> {
    const MARCA: &str = "Lucy:OK";
    let script = if h.protocol.os() == "windows" {
        "\"Lucy:OK|$((Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue).Caption)\""
    } else {
        // `PRETTY_NAME` está en cualquier Linux con systemd —a efectos prácticos,
        // cualquiera al que se llegue por SSH—; `uname` es el respaldo.
        ". /etc/os-release 2>/dev/null; echo \"Lucy:OK|${PRETTY_NAME:-$(uname -sr)}\""
    };
    let t0 = std::time::Instant::now();
    let (out, err, ok) = run_remote(h, password, script)?;
    let ms = t0.elapsed().as_millis() as u64;
    if let Some(l) = out.lines().find(|l| l.contains(MARCA)) {
        let os = l.split('|').nth(1).unwrap_or("").trim().to_string();
        return Ok(Probe { os: truncar(&os, 80), ms });
    }
    Err(motivo_de(&err, ok))
}

/// QuÃ© sistema corre en el equipo remoto. Una vista de `probe`.
///
/// TenÃ­a su propia conexiÃ³n y era un viaje de mÃ¡s: lo que hace falta para medir
/// el sistema es exactamente lo que hace falta para saber si se llega, y cada
/// conexiÃ³n de WinRM paga su autenticaciÃ³n, que es la parte lenta.
pub fn detect_os(h: &Host, password: &str) -> Option<String> {
    probe(h, password).ok().map(|p| p.os).filter(|s| !s.is_empty())
}

/// Comprueba que se llega al equipo y que las credenciales valen.
pub fn test_connection(h: &Host, password: &str) -> Result<u64, String> {
    probe(h, password).map(|p| p.ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un índice escrito por la app real, con sus nombres de campo.
    const DE_LA_APP: &str = r##"[
      {"id":"h_1730000000000","name":"Prod-Web-01","type":"windows","protocol":"winrm",
       "host":"192.168.1.10","username":"CORP\\admin","port":5985,"sshKeyPath":"",
       "tags":["prod","web"],"color":"#10b981","category":"shell"},
      {"id":"h_1730000000001","name":"db-1","type":"linux","protocol":"postgres",
       "host":"10.0.0.5","username":"postgres","port":5432,"sshKeyPath":"",
       "tags":["db"],"color":"#3b82f6","category":"database","dbType":"postgres"}
    ]"##;

    #[test]
    fn se_lee_lo_que_escribe_la_app() {
        // Si esto se rompe, las dos interfaces dejan de ver los mismos equipos —
        // en silencio, porque serde rellena con valores por defecto que casi
        // siempre parecen razonables.
        let v = parse(DE_LA_APP);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "Prod-Web-01");
        assert_eq!(v[0].protocol, Protocol::Winrm);
        assert_eq!(v[0].os, "windows");
        assert_eq!(v[0].port, 5985);
        assert_eq!(v[0].tags, vec!["prod", "web"]);
        assert_eq!(v[1].protocol, Protocol::Postgres);
        assert_eq!(v[1].db_type.as_deref(), Some("postgres"));
    }

    #[test]
    fn lo_que_se_escribe_se_puede_volver_a_leer_igual() {
        let v = parse(DE_LA_APP);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(parse(&json), v, "el viaje de ida y vuelta pierde algo");
        // Y los nombres del JSON son los de la app, no los de Rust.
        assert!(json.contains(r#""type":"windows""#), "{json}");
        assert!(json.contains(r#""dbType":"postgres""#), "{json}");
    }

    #[test]
    fn un_indice_ilegible_no_tumba_la_vista() {
        // Sin equipos remotos el resto funciona igual. Un `unwrap` aquí cambia
        // una función que falta por una pantalla que no arranca.
        assert!(parse("{esto no es json").is_empty());
        assert!(parse("").is_empty());
        assert!(parse("null").is_empty());
    }

    #[test]
    fn un_campo_que_la_app_no_escribio_toma_un_valor_sensato() {
        // Los índices viejos no tienen `protocol` ni `category`: los añadió una
        // versión posterior. Tienen que seguir leyéndose.
        let viejo = r#"[{"id":"h_1","name":"viejo","type":"linux","host":"10.0.0.1",
                         "username":"root"}]"#;
        let v = parse(viejo);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].protocol, Protocol::Winrm, "el respaldo del protocolo");
        assert_eq!(v[0].category, Category::Shell);
        assert_eq!(v[0].color, COLORS[0]);
    }

    #[test]
    fn el_protocolo_arrastra_puerto_sistema_y_categoria() {
        let mut h = Host::nuevo(Protocol::Winrm, 1);
        assert_eq!(h.port, 5985);
        h.set_protocol(Protocol::Ssh);
        assert_eq!(h.port, 22);
        assert_eq!(h.os, "linux");
        h.set_protocol(Protocol::Mssql);
        assert_eq!(h.port, 1433);
        assert_eq!(h.os, "windows");
        assert_eq!(h.category, Category::Database);
        assert_eq!(h.db_type.as_deref(), Some("mssql"));
    }

    #[test]
    fn un_puerto_puesto_a_mano_no_se_pisa() {
        // 5986 es WinRM sobre HTTPS, y quien lo escribe sabe lo que hace.
        // Cambiar de protocolo no puede borrarle esa decisión.
        let mut h = Host::nuevo(Protocol::Winrm, 1);
        h.port = 5986;
        h.set_protocol(Protocol::Rdp);
        assert_eq!(h.port, 5986, "le pisó un puerto deliberado");
    }

    #[test]
    fn faltan_todos_los_campos_de_una_vez() {
        // Rellenar uno, pulsar, y que te digan que falta otro es la forma más
        // lenta de completar un formulario.
        let h = Host::nuevo(Protocol::Winrm, 1);
        assert_eq!(h.missing(), vec!["nombre", "dirección", "usuario"]);
    }

    #[test]
    fn a_un_snmp_no_se_le_exige_usuario() {
        // No lo tiene. Exigirlo impediría dar de alta media infraestructura.
        let mut h = Host::nuevo(Protocol::Snmp, 1);
        h.name = "switch-1".into();
        h.host = "10.0.0.2".into();
        assert!(h.missing().is_empty(), "{:?}", h.missing());
    }

    #[test]
    fn solo_se_ofrece_terminal_donde_hay_terminal() {
        // Están en la lista porque son parte del parque y se quieren apuntar;
        // abrirles una consola no significa nada.
        assert!(Protocol::Winrm.can_shell());
        assert!(Protocol::Ssh.can_shell());
        for p in [Protocol::Redis, Protocol::K8s, Protocol::Snmp, Protocol::Rdp] {
            assert!(!p.can_shell(), "{p:?} no tiene shell");
        }
    }

    #[test]
    fn la_contrasena_no_aparece_en_el_script() {
        // NUNCA interpolada. En los argumentos la vería cualquiera con un
        // `Get-Process`; dentro del literal de PowerShell, una contraseña con
        // una comilla cierra la cadena y lo de detrás se ejecuta — la inyección
        // S1 del informe de seguridad de la app.
        let mut h = Host::nuevo(Protocol::Winrm, 1);
        h.host = "srv".into();
        h.username = "admin".into();
        let w = winrm_wrapper(&h, "Get-Service");
        assert!(!w.contains("ClaveSuperSecreta"), "no se le pasó, pero por si acaso");
        assert!(w.contains("[Console]::In.ReadLine()"), "no la lee de la entrada: {w}");
        // Y el script del operador tampoco va en claro: va en base64, para que
        // no pueda cerrar el ScriptBlock que lo envuelve.
        assert!(!w.contains("Get-Service"), "el script viaja sin codificar: {w}");
        assert!(w.contains("FromBase64String"));
    }

    #[test]
    fn una_comilla_en_el_usuario_no_rompe_el_script() {
        // Un usuario con apóstrofo es raro pero legal, y sin doblarlo cierra el
        // literal igual que lo haría una contraseña.
        let mut h = Host::nuevo(Protocol::Winrm, 1);
        h.host = "srv".into();
        h.username = "d'arcy".into();
        let w = winrm_wrapper(&h, "x");
        assert!(w.contains("d''arcy"), "no dobló la comilla: {w}");
    }

    #[test]
    fn el_puerto_configurado_llega_al_comando() {
        // 5986 es WinRM sobre HTTPS. Ignorarlo aquí haría que un equipo bien
        // dado de alta fallara con un error de red.
        let mut h = Host::nuevo(Protocol::Winrm, 1);
        h.host = "srv".into();
        h.port = 5986;
        assert!(winrm_wrapper(&h, "x").contains("-Port 5986"));
    }

    #[test]
    fn un_error_partido_por_la_consola_se_vuelve_a_juntar() {
        // EL FALLO QUE ESTO ARREGLA. Se cogía la primera línea, «la que dice qué
        // pasó», y esa premisa es falsa: PowerShell parte los mensajes largos al
        // ancho del búfer. El operador leía «El cliente WinRM no puede» y ahí se
        // acababa — justo antes de la parte que dice qué hacer.
        let real = "[192.168.1.100] Error de conexión al servidor remoto 192.168.1.100. \
                    Mensaje de error: El cliente WinRM no puede\nprocesar la solicitud. No se \
                    puede usar la autenticación Kerberos.\n";
        let m = motivo_de(real, false);
        assert!(m.contains("El cliente WinRM no puede procesar"), "{m}");
        assert!(m.contains("Kerberos"), "se perdió el final: {m}");
    }

    #[test]
    fn el_rastro_de_pila_no_entra() {
        // Se descarta por lo que ES —las líneas de `+ CategoryInfo` y compañía—
        // y no por venir después, que era la regla equivocada.
        let e = "No se puede conectar al host.\n    + CategoryInfo : OpenError\n    \
                 + FullyQualifiedErrorId : CannotConnect";
        let m = motivo_de(e, false);
        assert_eq!(m, "No se puede conectar al host.");
    }

    #[test]
    fn sin_nada_que_decir_se_distingue_no_contesto_de_contesto_raro() {
        // Son dos problemas distintos y llevan a mirar sitios distintos: la red
        // por un lado, el propio comando por el otro.
        assert!(motivo_de("", false).contains("sin respuesta"));
        assert!(motivo_de("   \n  ", true).contains("no reconozco"));
    }

    #[test]
    fn el_envoltorio_devuelve_el_mensaje_limpio_y_no_el_formateado() {
        // Sin el try/catch, PowerShell formatea el error para una consola: lo
        // parte y le añade sus dos líneas de categoría. Con él llega
        // `$_.Exception.Message`, entero y en una pieza.
        let mut h = Host::nuevo(Protocol::Winrm, 1);
        h.host = "srv".into();
        let w = winrm_wrapper(&h, "x");
        assert!(w.contains("$_.Exception.Message"), "{w}");
        assert!(w.contains("[Console]::Error.WriteLine"), "no lo manda por la salida de error");
    }

    #[test]
    fn ssh_se_lanza_directo_y_no_dentro_de_powershell() {
        // Pasaba por PowerShell solo para que PowerShell llamara a `ssh`. Esa
        // capa arriesgaba el entrecomillado y, sobre todo, se comía la entrada
        // estándar — la única forma de contestarle a un `sudo`.
        let mut h = Host::nuevo(Protocol::Ssh, 1);
        h.host = "10.0.0.5".into();
        h.username = "root".into();
        let a = ssh_args(&h, "uptime");
        assert!(a.iter().any(|x| x == "root@10.0.0.5"), "{a:?}");
        assert!(a.iter().any(|x| x == "-p"), "no pasa el puerto: {a:?}");
        // Y el comando remoto va codificado, no en claro: su contenido no puede
        // cerrar ninguna comilla.
        assert!(!a.iter().any(|x| x == "uptime"), "el comando viaja sin codificar: {a:?}");
        assert!(a.iter().any(|x| x.contains("base64 -d")), "{a:?}");
    }

    #[test]
    fn una_clave_con_espacios_no_necesita_comillas_en_argv() {
        // Es la mitad del valor de pasar argv en vez de una línea de shell: la
        // ruta viaja como UN argumento y nadie tiene que escaparla.
        let mut h = Host::nuevo(Protocol::Ssh, 1);
        h.host = "srv".into();
        h.username = "u".into();
        h.ssh_key_path = r"C:\mis claves\id_ed25519".into();
        let a = ssh_args(&h, "x");
        let i = a.iter().position(|x| x == "-i").expect("sin -i");
        assert_eq!(a[i + 1], r"C:\mis claves\id_ed25519");
    }

    #[test]
    fn una_maquina_nueva_se_acepta_pero_un_cambio_de_clave_no() {
        // `accept-new` y no `no`: lo primero es comodidad, lo segundo apaga el
        // aviso de que la clave de un servidor conocido CAMBIÓ, que es la señal
        // que importa.
        let mut h = Host::nuevo(Protocol::Ssh, 1);
        h.host = "srv".into();
        h.username = "u".into();
        let a = ssh_args(&h, "x").join(" ");
        assert!(a.contains("StrictHostKeyChecking=accept-new"), "{a}");
        assert!(!a.contains("StrictHostKeyChecking=no"), "apagó el aviso: {a}");
    }

    #[test]
    fn contra_lo_que_no_tiene_shell_no_se_intenta() {
        // Se dice, en vez de fallar con un error de red que no explica nada.
        let mut h = Host::nuevo(Protocol::Redis, 1);
        h.host = "cache".into();
        let e = run_remote(&h, "", "x").unwrap_err();
        assert!(e.contains("Redis"), "{e}");
    }

    #[test]
    fn cada_protocolo_tiene_su_puerto_y_su_grupo() {
        // Nadie recuerda que WinRM es 5985 y RDP 3389, y equivocarse produce un
        // fallo de red que no se parece a "te has equivocado de puerto".
        for p in Protocol::ALL {
            assert!(p.default_port() > 0, "{p:?} sin puerto");
            assert!(!p.label().is_empty());
            assert!(!p.group().is_empty());
            assert_eq!(Protocol::from_key(p.key()), Some(p), "{p:?} no vuelve de su clave");
        }
    }
}
