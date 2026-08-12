//! Inventario de un equipo: qué escucha, qué corre, qué hay instalado, qué
//! caduca y qué se dispara solo.
//!
//! Es una FOTO, no un panel de vigilancia: se pide, se lee y se cierra. Por eso
//! aquí no hay temporizadores ni instantáneas guardadas — la V2 reescaneaba cada
//! treinta minutos mientras la vista estuviera abierta, con un PowerShell de
//! varios segundos por vuelta, para un dato que cambia cuando alguien instala
//! algo.
//!
//! LA SALIDA NO ES JSON, Y ESA ES LA DECISIÓN DEL MÓDULO. La V2 fabrica el JSON
//! del lado de Linux a mano, con `echo` y `printf`, y eso trae tres problemas que
//! no son teóricos:
//!
//! · Para que su JSON casero no reviente, BORRA caracteres del dato: `tr -d '"\'`
//!   sobre el asunto de un certificado, `gsub(/"/, ...)` sobre la descripción de
//!   un servicio. O sea que `CN="Acme, Inc."` llega mutilado y `C:\Program
//!   Files\Node.js` pierde las barras.
//! · Recorta con `head -c 8000`, que corta por BYTES en medio de un objeto. El
//!   documento entero deja de ser JSON válido, y como se deserializa de una pieza
//!   se cae el inventario COMPLETO —puertos incluidos— en cualquier Debian con
//!   más de ciento ochenta paquetes.
//! · Una sección que no produce nada deja `[]` o `[,]` según el camino, y el
//!   segundo tampoco es JSON.
//!
//! Aquí cada registro es una línea con sus campos separados por US (0x1F) y un
//! prefijo que dice de qué sección es. No hay nada que escapar, así que no hay
//! nada que borrar del dato; una línea rota se descarta sola sin llevarse a las
//! demás; y el recorte se hace en Rust, por registros, donde se puede decir
//! cuántos se dejaron fuera.
//!
//! EL SEPARADOR ES US Y NO LA BARRA VERTICAL, y viene de un caso real: una
//! entrada de cron perfectamente normal es
//! `0 3 * * * /bin/backup | mail -s ok root`. Con barra vertical esa línea se
//! parte en campos fantasma y la tarea aparece cortada justo por la mitad que
//! dice qué hace.

/// El separador de campos: US, «unit separator».
///
/// Se eligió del bloque de control de ASCII porque no aparece en un nombre de
/// servicio, en una ruta ni en una línea de cron. Cualquier carácter imprimible
/// —la barra vertical, el punto y coma, la tabulación— sale antes o después en
/// un dato real.
pub const US: char = '\u{1f}';

/// El prefijo que marca una línea nuestra.
///
/// Existe porque la salida NO viene limpia: PowerShell escribe avisos por su
/// cuenta y `ss`/`find` mandan errores de permisos que en WinRM llegan mezclados
/// con lo bueno. Sin una marca, «Permission denied» entraría en la tabla como si
/// fuera un puerto.
pub const MARCA: &str = "LUCY:";

// ── Topes ───────────────────────────────────────────────────────────────────
//
// SE APLICAN AQUÍ Y NO EN EL EQUIPO REMOTO. Es la diferencia entre recortar una
// lista y corromper el documento: `head -c 8000` corta por bytes en mitad de un
// registro. Y en Rust se puede decir cuántos quedaron fuera, que es lo que
// convierte «hay 40 puertos» en «hay 40 de 312».
//
// Los números salen de para qué se mira cada lista. Cincuenta puertos era el
// tope de la V2 y se queda corto en el primer servidor con un firewall delante;
// el software es la lista que alimenta el cruce de vulnerabilidades, así que
// recortarla esconde justo lo que hay que encontrar.
pub const MAX_PORTS: usize = 200;
/// Un Windows 11 de escritorio normal tiene 307 servicios. Con el tope en 300 se
/// caían siete SIEMPRE LOS MISMOS —la cola del alfabeto, `wuauserv` entre
/// ellos, que está arrancado— y el aviso de recorte no ayudaba: nadie sospecha
/// que le falte justo Windows Update.
pub const MAX_SERVICES: usize = 800;
pub const MAX_SOFTWARE: usize = 400;
pub const MAX_CERTS: usize = 50;
/// Un Windows 11 de escritorio normal tiene 205 tareas programadas — medido
/// ejecutando el script de este módulo contra esta máquina. Con el tope en 100
/// se recortaba la mitad, y aunque el aviso lo decía, la mitad que sobrevive de
/// una lista alfabética no es una muestra de nada.
pub const MAX_TASKS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Categoria {
    Puertos,
    Servicios,
    Software,
    Certificados,
    Tareas,
}

impl Categoria {
    pub fn label(self) -> &'static str {
        match self {
            Self::Puertos => "Puertos",
            Self::Servicios => "Servicios",
            Self::Software => "Software",
            Self::Certificados => "Certificados",
            Self::Tareas => "Tareas",
        }
    }

    /// El nombre corto que va en la línea de salida.
    ///
    /// Público porque es parte del formato de la salida, no un detalle: quien
    /// mire un volcado crudo necesita saber que `ports` es Puertos.
    pub fn clave(self) -> &'static str {
        match self {
            Self::Puertos => "ports",
            Self::Servicios => "services",
            Self::Software => "software",
            Self::Certificados => "certs",
            Self::Tareas => "tasks",
        }
    }

    fn de_clave(s: &str) -> Option<Self> {
        Some(match s {
            "ports" => Self::Puertos,
            "services" => Self::Servicios,
            "software" => Self::Software,
            "certs" => Self::Certificados,
            "tasks" => Self::Tareas,
            _ => return None,
        })
    }

    pub const ALL: [Self; 5] = [
        Self::Puertos,
        Self::Servicios,
        Self::Software,
        Self::Certificados,
        Self::Tareas,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub struct Port {
    pub port: u32,
    pub process: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub name: String,
    pub status: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Software {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cert {
    pub path: String,
    pub subject: String,
    /// Cuándo caduca, en epoch de SEGUNDOS. `None` = no se pudo averiguar.
    ///
    /// EL `None` ES OBLIGATORIO Y NO UN LUJO. Con un `i64` a secas, el cero
    /// significa dos cosas incompatibles: «caduca el 1 de enero de 1970» y «el
    /// equipo no supo darme la fecha». Y la segunda pasa de verdad — en Alpine y
    /// en BSD, `date -d` no entiende el formato que escupe `openssl x509`. La
    /// fila salía perfectamente formada, y la vista pintaba en rojo «caducó hace
    /// 20672d» sobre un certificado que a lo mejor está impecable.
    ///
    /// El equipo remoto manda el instante y los días los calcula Rust. La V2 los
    /// calculaba allí, en cada sistema por su cuenta —`(NotAfter - Get-Date).Days`
    /// en PowerShell, una resta de epochs con `date -d` en bash— y las dos
    /// cuentas redondean distinto: un certificado que caduca esta noche sale como
    /// «0 días» en un sitio y «1 día» en el otro. Un solo reloj y una sola resta.
    pub expires_epoch: Option<i64>,
}

impl Cert {
    /// Días que le quedan. Negativo = ya caducó. `None` = no se sabe cuándo.
    pub fn days_left(&self, ahora: i64) -> Option<i64> {
        // División entera hacia abajo también con negativos: `-1/86400` en Rust
        // da 0, y un certificado que caducó hace una hora saldría como «le quedan
        // 0 días» en vez de como caducado.
        Some((self.expires_epoch? - ahora).div_euclid(86_400))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub entry: String,
    /// `Ready`, `Running`, `Disabled`… Vacío en cron, que no tiene estado.
    ///
    /// LA V2 FILTRABA POR `State -eq 'Ready'` y por eso las tareas que estaban
    /// EJECUTÁNDOSE en ese instante no existían para el inventario: se escanea,
    /// no aparecen; se reescanea treinta segundos después y aparecen. La misma
    /// máquina daba dos respuestas y ninguna decía por qué. Ahora vienen todas
    /// con su estado, y quien quiera filtrar lo hace mirándolo.
    pub state: String,
}

/// La foto completa de un equipo.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inventory {
    pub ports: Vec<Port>,
    pub services: Vec<Service>,
    pub software: Vec<Software>,
    pub certs: Vec<Cert>,
    pub tasks: Vec<Task>,
    /// Las secciones que fallaron, con su motivo.
    ///
    /// POR SECCIÓN Y NO UNA SOLA BANDERA. `Get-ScheduledTask` necesita permisos
    /// que `Get-Service` no, así que en un equipo donde Lucy entra sin ser
    /// administrador fallan las tareas y funciona todo lo demás. Un error global
    /// tiraría las cuatro listas buenas por culpa de la quinta.
    pub fallos: Vec<(Categoria, String)>,
    /// Lo que se recortó: `(categoría, cuántos había de verdad)`.
    ///
    /// Una lista recortada en silencio se lee como una lista completa, y sobre
    /// inventario eso es peor que no tenerla: se concluye que un paquete no está
    /// instalado cuando lo que pasa es que no cupo.
    pub truncado: Vec<(Categoria, usize)>,
    /// El transporte se cortó a media foto, con el motivo.
    ///
    /// UN ESCANEO INTERRUMPIDO NO PUEDE ENSEÑARSE COMO COMPLETO. Si la sesión se
    /// cae después de los puertos —`Connection closed by remote host`, o WinRM
    /// tirando el `Invoke-Command` por su propio plazo—, llegan cuarenta líneas
    /// buenas y ninguna de error. Con solo mirar «¿hay datos?» eso pasaba por un
    /// éxito, y el operador leía «Servicios (0)» como un hecho del servidor.
    pub parcial: Option<String>,
}

impl Inventory {
    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
            && self.services.is_empty()
            && self.software.is_empty()
            && self.certs.is_empty()
            && self.tasks.is_empty()
    }

    pub fn len_de(&self, c: Categoria) -> usize {
        match c {
            Categoria::Puertos => self.ports.len(),
            Categoria::Servicios => self.services.len(),
            Categoria::Software => self.software.len(),
            Categoria::Certificados => self.certs.len(),
            Categoria::Tareas => self.tasks.len(),
        }
    }

    pub fn fallo_de(&self, c: Categoria) -> Option<&str> {
        self.fallos.iter().find(|(k, _)| *k == c).map(|(_, m)| m.as_str())
    }
}

/// El script de descubrimiento para un equipo Windows.
///
/// Vale igual para el local y para WinRM: lo único que cambia entre los dos es el
/// transporte. En la V2 eran dos copias del mismo texto en el mismo fichero, y
/// dos copias acaban discrepando el día que alguien arregla una.
pub fn windows_script() -> String {
    // La consulta de software sale de UNA sola constante. En la V2 estaba
    // duplicada, y su comentario cuenta cómo acabó: «un aviso CRITICAL al lado de
    // un panel de Inventario marcando 0», porque las dos copias miraban claves
    // distintas del registro.
    format!(
        "$ErrorActionPreference='Continue'\n\
         function W($s,$f){{ Write-Output ('{MARCA}'+$s+[char]31+$f) }}\n\
         function E($s,$m){{ Write-Output ('{MARCA}err'+[char]31+$s+[char]31+$m) }}\n\
         try {{ Get-NetTCPConnection -State Listen -EA Stop | Group-Object LocalPort | \
           ForEach-Object {{ $p=$_.Name; \
             $pn=(Get-Process -Id ($_.Group|Select-Object -First 1).OwningProcess -EA SilentlyContinue).Name; \
             W 'ports' ([string]$p+[char]31+[string]$pn) }} }} catch {{ E 'ports' $_.Exception.Message }}\n\
         try {{ Get-Service -EA Stop | ForEach-Object {{ \
             W 'services' ($_.Name+[char]31+$_.Status.ToString()+[char]31+$_.DisplayName) }} }} \
           catch {{ E 'services' $_.Exception.Message }}\n\
         try {{ {software} | ForEach-Object {{ \
             W 'software' ($_.name+[char]31+$_.version) }} }} catch {{ E 'software' $_.Exception.Message }}\n\
         try {{ Get-ChildItem Cert:\\LocalMachine\\My -EA Stop | ForEach-Object {{ \
             W 'certs' ('Cert:\\LocalMachine\\My'+[char]31+$_.Subject+[char]31+\
             [string][int64]($_.NotAfter.ToUniversalTime()-[datetime]'1970-01-01').TotalSeconds) }} }} \
           catch {{ E 'certs' $_.Exception.Message }}\n\
         try {{ Get-ScheduledTask -EA Stop | \
           ForEach-Object {{ W 'tasks' ($_.TaskPath+$_.TaskName+[char]31+$_.State.ToString()) }} }} \
           catch {{ E 'tasks' $_.Exception.Message }}",
        software = INSTALLED_SOFTWARE_PS
    )
}

/// La consulta de software instalado, hermana de `cve_match::INSTALLED_SOFTWARE_PS`.
///
/// Aquí y no importada porque `lucy-core` no depende de la app Tauri —es al
/// revés—, y el comentario de allí explica adónde lleva que dos copias difieran:
/// «un aviso CRITICAL al lado de un panel de Inventario marcando 0». Se lee de
/// las DOS ramas del registro: sin `Wow6432Node`, en un Windows de 64 bits
/// desaparece todo el software de 32.
///
/// DIFIERE DE LA DE ALLÍ EN UNA COSA, Y A PROPÓSITO: aquella lleva
/// `Select-Object -First 250` y ésta no. Recortar en el equipo remoto esconde
/// software del cruce de vulnerabilidades sin que nadie pueda saberlo —la lista
/// llega corta y parece completa—, así que el tope se aplica en Rust, donde se
/// puede decir cuántos quedaron fuera. Si alguien alinea las dos copias, que sea
/// quitando el recorte de allá y no poniéndolo aquí.
pub const INSTALLED_SOFTWARE_PS: &str = "Get-ItemProperty \
     HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*,\
     HKLM:\\Software\\Wow6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*,\
     HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*,\
     HKCU:\\Software\\Wow6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\* \
     -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName } | \
     Sort-Object DisplayName -Unique | \
     ForEach-Object { [PSCustomObject]@{name=$_.DisplayName; version=$_.DisplayVersion} }";

/// El script de descubrimiento para un equipo Linux.
pub fn linux_script() -> String {
    // Sin `head -c`: el recorte por bytes es lo que corrompía el documento. Aquí
    // se manda todo y recorta Rust, que además puede decir cuántos había.
    format!(
        // SE MIRA EL RESULTADO, NO SI EXISTE EL BINARIO. `command -v ss` dice que
        // sí en un RHEL 7 cuyo `ss` no conoce `-H`; el comando sale con error de
        // uso, el `2>/dev/null` se lo come y la pestaña dice «Puertos (0)» sin un
        // solo fallo anotado. Capturar la salida CON su error y mirar el código
        // de salida convierte eso en un motivo que se puede leer.
        //
        // Y `-p` en `ss`: la columna del proceso solo la imprime con esa opción.
        // Sin ella, `users:((...))` no aparece nunca y el campo salía vacío en
        // TODOS los Linux — por construcción, no por permisos.
        "S=$(printf '\\037')\n\
         o=$(ss -tlnp 2>&1); \
         if [ $? -eq 0 ]; then \
           printf '%s\\n' \"$o\" | awk -v S=\"$S\" 'NR>1 || $1!=\"State\" {{ \
             if ($1==\"State\") next; \
             split($4,a,\":\"); p=a[length(a)]; \
             proc=\"\"; if (match($0, /users:\\(\\(\"[^\"]+\"/)) {{ \
               proc=substr($0, RSTART+9, RLENGTH-10) }}; \
             if (p ~ /^[0-9]+$/) printf \"{MARCA}ports%s%s%s%s\\n\", S, p, S, proc }}'; \
         else printf '{MARCA}err%sports%s%s\\n' \"$S\" \"$S\" \"$o\"; fi\n\
         o=$(systemctl list-units --type=service --all --no-pager --no-legend --plain 2>&1); \
         if [ $? -eq 0 ]; then \
           printf '%s\\n' \"$o\" | awk -v S=\"$S\" '{{ n=$1; sub(/\\.service$/,\"\",n); \
             if (n==\"\") next; d=\"\"; \
             for(i=5;i<=NF;i++) d=d (i>5?\" \":\"\") $i; \
             printf \"{MARCA}services%s%s%s%s%s%s\\n\", S, n, S, $4, S, d }}'; \
         else printf '{MARCA}err%sservices%s%s\\n' \"$S\" \"$S\" \"$o\"; fi\n\
         if command -v dpkg-query >/dev/null 2>&1; then \
           dpkg-query -W -f=\"{MARCA}software$S\\${{Package}}$S\\${{Version}}\\n\" 2>/dev/null; \
         elif command -v rpm >/dev/null 2>&1; then \
           rpm -qa --queryformat \"{MARCA}software$S%{{NAME}}$S%{{VERSION}}\\n\" 2>/dev/null; \
         elif command -v apk >/dev/null 2>&1; then \
           apk list --installed 2>/dev/null | awk -v S=\"$S\" '{{ \
             n=$1; sub(/-[^-]*-[^-]*$/,\"\",n); printf \"{MARCA}software%s%s%s\\n\", S, n, S }}'; \
         else printf '{MARCA}err%ssoftware%sni dpkg ni rpm ni apk\\n' \"$S\" \"$S\"; fi\n\
         find -L /etc/letsencrypt/live /etc/pki/tls/certs /etc/nginx/ssl /etc/apache2/ssl \
           \\( -name '*.pem' -o -name '*.crt' \\) -type f 2>/dev/null | \
           while IFS= read -r f; do \
             e=$(openssl x509 -enddate -noout -in \"$f\" 2>/dev/null | cut -d= -f2); \
             [ -z \"$e\" ] && continue; \
             s=$(openssl x509 -subject -noout -in \"$f\" 2>/dev/null | sed 's/^subject= *//'); \
             ep=$(date -d \"$e\" +%s 2>/dev/null || date -j -f '%b %e %T %Y %Z' \"$e\" +%s 2>/dev/null || echo ''); \
             printf '{MARCA}certs%s%s%s%s%s%s\\n' \"$S\" \"$f\" \"$S\" \"$s\" \"$S\" \"$ep\"; \
           done\n\
         (crontab -l 2>/dev/null; cat /etc/cron.d/* 2>/dev/null) | \
           grep -v '^[[:space:]]*#' | grep -v '^[[:space:]]*$' | \
           while IFS= read -r l; do printf '{MARCA}tasks%s%s%s\\n' \"$S\" \"$l\" \"$S\"; done"
    )
}

/// El script que le toca a este equipo, o por qué no se puede inventariar.
pub fn remote_script(h: &crate::hosts::Host) -> Result<String, String> {
    if !h.protocol.can_shell() {
        return Err(format!(
            "«{}» está dado de alta como {} y por ahí no se puede inventariar. Hace \
             falta WinRM o SSH.",
            h.name,
            h.protocol.label()
        ));
    }
    Ok(if h.protocol == crate::hosts::Protocol::Winrm {
        windows_script()
    } else {
        linux_script()
    })
}

/// Interpreta una línea de la salida. `None` = no es nuestra, o está rota.
///
/// Devolver `None` en vez de un error es deliberado: la salida trae de todo
/// —avisos de PowerShell, errores de permisos de `find`— y cada línea ajena es
/// normal, no un fallo. Lo que no puede pasar es que una de ellas acabe en la
/// tabla como si fuera un dato.
fn parse_linea(l: &str, inv: &mut Inventory) {
    let Some(resto) = l.trim_end_matches('\r').strip_prefix(MARCA) else { return };
    let mut campos = resto.split(US);
    let Some(seccion) = campos.next() else { return };

    if seccion == "err" {
        let (Some(cat), Some(msg)) = (campos.next(), campos.next()) else { return };
        if let Some(c) = Categoria::de_clave(cat) {
            let msg = msg.trim();
            inv.fallos.push((
                c,
                if msg.is_empty() { "no se pudo consultar".to_string() } else { msg.to_string() },
            ));
        }
        return;
    }

    match Categoria::de_clave(seccion) {
        Some(Categoria::Puertos) => {
            let Some(p) = campos.next().and_then(|s| s.trim().parse::<u32>().ok()) else { return };
            inv.ports.push(Port {
                port: p,
                process: limpia(campos.next().unwrap_or_default()),
            });
        }
        Some(Categoria::Servicios) => {
            let (Some(n), Some(s)) = (campos.next(), campos.next()) else { return };
            let n = limpia(n);
            if n.is_empty() {
                return;
            }
            inv.services.push(Service {
                name: n,
                status: limpia(s).to_lowercase(),
                description: limpia(campos.next().unwrap_or_default()),
            });
        }
        Some(Categoria::Software) => {
            let Some(n) = campos.next().map(limpia).filter(|s| !s.is_empty()) else { return };
            inv.software.push(Software {
                name: n,
                // Un paquete sin versión es normal —los hay— y descartarlo por
                // eso escondería software instalado.
                version: limpia(campos.next().unwrap_or_default()),
            });
        }
        Some(Categoria::Certificados) => {
            let (Some(path), Some(subject), Some(ep)) =
                (campos.next(), campos.next(), campos.next())
            else {
                return;
            };
            inv.certs.push(Cert {
                path: limpia(path),
                subject: limpia(subject),
                // `.ok()` y no `.unwrap_or(0)`: un campo vacío o ilegible es «no
                // se sabe», y convertirlo en cero lo volvía «caducó en 1970».
                expires_epoch: ep.trim().parse().ok(),
            });
        }
        Some(Categoria::Tareas) => {
            let e = limpia(campos.next().unwrap_or_default());
            if !e.is_empty() {
                inv.tasks.push(Task {
                    entry: e,
                    state: limpia(campos.next().unwrap_or_default()),
                });
            }
        }
        None => {}
    }
}

/// Neutraliza los caracteres de control que sobrevivan al transporte.
///
/// Importan: un nombre con `\u{7}` dentro hace sonar la campana del terminal al
/// copiar la tabla, y uno con `\u{1b}` mete una secuencia de escape en el
/// portapapeles.
///
/// SE SUSTITUYEN POR UN ESPACIO, NO SE BORRAN — y esa diferencia era un fallo
/// real. `/etc/cron.d/*` separa sus campos con TABULADOR:
/// `30 4 * * 1<TAB>root<TAB>/usr/local/bin/backup.sh`. Borrándolo salía
/// `30 4 * * 1root/usr/local/bin/backup.sh`, que no se puede leer ni buscar. El
/// tabulador es Cc igual que la campana, así que el filtro se los llevaba a los
/// dos por igual.
fn limpia(s: &str) -> String {
    let sin_control: String =
        s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    // Y se colapsan las repeticiones: dos tabuladores seguidos dejarían dos
    // espacios donde el original tenía una separación.
    sin_control.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Interpreta la salida entera y aplica los topes.
pub fn parse(salida: &str) -> Inventory {
    let mut inv = Inventory::default();
    for l in salida.lines() {
        parse_linea(l, &mut inv);
    }
    recorta(&mut inv);
    inv
}

fn recorta(inv: &mut Inventory) {
    fn corta<T>(v: &mut Vec<T>, max: usize, cat: Categoria, out: &mut Vec<(Categoria, usize)>) {
        if v.len() > max {
            out.push((cat, v.len()));
            v.truncate(max);
        }
    }
    let mut t = Vec::new();
    corta(&mut inv.ports, MAX_PORTS, Categoria::Puertos, &mut t);
    corta(&mut inv.services, MAX_SERVICES, Categoria::Servicios, &mut t);
    corta(&mut inv.software, MAX_SOFTWARE, Categoria::Software, &mut t);
    corta(&mut inv.certs, MAX_CERTS, Categoria::Certificados, &mut t);
    corta(&mut inv.tasks, MAX_TASKS, Categoria::Tareas, &mut t);
    inv.truncado = t;
}

/// Inventaría este equipo.
pub fn discover_local() -> Result<Inventory, String> {
    let (out, err, ok) = crate::shell::run_powershell_utf8(&windows_script())?;
    cierra(parse(&out), &err, ok, "este equipo")
}

/// Cuánto se espera a un equipo antes de darlo por perdido.
///
/// Dos minutos. Un inventario contra un servidor con mucho software tarda entre
/// diez y treinta segundos, así que esto no corta nada legítimo; lo que corta es
/// el caso en que no hay nadie al otro lado. Sin plazo, un WinRM apagado deja la
/// vista muerta los MINUTOS que tarde `Invoke-Command` en rendirse — y durante
/// todo ese rato no hay nada que pulsar.
pub const TIMEOUT_SECS: u64 = 120;

/// Inventaría un equipo remoto. Se puede parar y tiene plazo.
pub fn discover_remote(
    h: &crate::hosts::Host,
    password: &str,
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Inventory, String> {
    use std::sync::atomic::Ordering;
    let script = remote_script(h)?;
    let (tx, rx) = std::sync::mpsc::channel();
    // Por el camino en STREAMING y no por `run_remote`, que bloquea en
    // `wait_with_output()` sin plazo ni forma de interrumpirlo. Aquí no se usa
    // el goteo para enseñar nada —el inventario se pinta entero— sino para poder
    // soltar el proceso cuando el operador lo pide o cuando se acaba el tiempo.
    crate::hosts::run_remote_streaming(h, password, &script, &tx, stop, None, Some(TIMEOUT_SECS))?;

    let mut out = String::new();
    let mut err = String::new();
    let mut ok = false;
    for l in rx {
        match l {
            crate::hosts::Line::Out(t) => {
                out.push_str(&t);
                out.push('\n');
            }
            crate::hosts::Line::Err(t) => {
                err.push_str(&t);
                err.push('\n');
            }
            crate::hosts::Line::Done(v) => ok = v,
        }
    }
    if stop.load(Ordering::Relaxed) {
        // Lo que llegó antes de parar NO se devuelve como si fuera la foto: es
        // media foto de la que nadie dijo que lo fuera. Un error se entiende;
        // media lista presentada como entera, no.
        return Err(format!(
            "El inventario de {} se detuvo antes de terminar (parado o sin respuesta en {}s).",
            h.name, TIMEOUT_SECS
        ));
    }
    cierra(parse(&out), &err, ok, &h.name)
}

/// Decide si lo que llegó vale, y si vale entero.
///
/// TRES DESENLACES Y NO DOS. `ok` por sí solo no sirve —el script atrapa cada
/// sección y sigue, así que un código de salida distinto de cero convive con
/// cuatro listas buenas—, pero ignorarlo cuando hay datos era peor: una sesión
/// que se corta a mitad devuelve las primeras secciones y ninguna marca de
/// error, y eso pasaba por una foto completa.
fn cierra(mut inv: Inventory, err: &str, ok: bool, equipo: &str) -> Result<Inventory, String> {
    let motivo = err.trim();
    if inv.is_empty() && inv.fallos.is_empty() {
        return Err(if !motivo.is_empty() {
            motivo.to_string()
        } else if ok {
            format!("{equipo} no devolvió ningún dato de inventario.")
        } else {
            format!("No se pudo inventariar {equipo}.")
        });
    }
    // Hay datos, pero el transporte se quejó: la foto puede estar a medias y hay
    // que decirlo en vez de dejar que las secciones vacías se lean como hechos.
    if !ok || !motivo.is_empty() {
        inv.parcial = Some(if motivo.is_empty() {
            format!("La sesión con {equipo} terminó con error; puede faltar información.")
        } else {
            motivo.to_string()
        });
    }
    Ok(inv)
}

/// El inventario en CSV, para llevárselo a una hoja de cálculo.
///
/// CSV Y NO PDF. El marcador de posición de la vista pedía PDF; un PDF necesita
/// un crate de maquetación, una fuente empotrada y decisiones de página, y lo que
/// se hace de verdad con un inventario es cruzarlo con otra lista —qué había el
/// mes pasado, qué dice el contrato de soporte— que es exactamente lo que un CSV
/// permite y un PDF no.
pub fn to_csv(inv: &Inventory, equipo: &str) -> String {
    // COLUMNAS CON NOMBRE. Decía `campo1,campo2,campo3`, y en una hoja de
    // cálculo eso obliga a deducir qué es cada una mirando la categoría de la
    // fila — que es justo lo que un export existe para evitar. Los nombres son
    // genéricos porque las cinco categorías comparten tabla, pero «valor» y
    // «detalle» se pueden leer; «campo2» no.
    let mut s = String::from("equipo,categoria,nombre,valor,detalle\n");
    let mut fila = |cat: &str, a: &str, b: &str, c: &str| {
        s.push_str(&format!(
            "{},{},{},{},{}\n",
            csv(equipo),
            cat,
            csv(a),
            csv(b),
            csv(c)
        ));
    };
    for p in &inv.ports {
        fila("puerto", &p.port.to_string(), &p.process, "");
    }
    for x in &inv.services {
        fila("servicio", &x.name, &x.status, &x.description);
    }
    for x in &inv.software {
        fila("software", &x.name, &x.version, "");
    }
    for x in &inv.certs {
        fila(
            "certificado",
            &x.subject,
            &x.expires_epoch.map(|e| e.to_string()).unwrap_or_else(|| "desconocida".into()),
            &x.path,
        );
    }
    for x in &inv.tasks {
        fila("tarea", &x.entry, &x.state, "");
    }
    s
}

/// Un campo de CSV, entrecomillado si le hace falta y desactivado si parece una
/// fórmula.
///
/// El asunto de un certificado lleva comas casi siempre —`CN=api, O=Acme, C=ES`—
/// así que sin el entrecomillado el fichero sale con las columnas desplazadas
/// justo en la tabla que más se exporta.
///
/// Y EL PREFIJO. Una tarea de cron perfectamente normal es
/// `@reboot /usr/local/bin/sync.sh`, y Excel y LibreOffice tratan `@`, `=`, `+`
/// y `-` como comienzo de fórmula al importar: la celda muestra `#NAME?` en
/// lugar de la tarea. Entrecomillar NO lo desactiva —es cosa del importador, no
/// del CSV— así que hay que romper el primer carácter con una comilla simple,
/// que las hojas de cálculo entienden como «esto es texto».
fn csv(s: &str) -> String {
    let formula = s.starts_with(['=', '+', '-', '@', '\t', '\r']);
    let cuerpo = if formula { format!("'{s}") } else { s.to_string() };
    if formula || cuerpo.contains([',', '"', '\n']) {
        format!("\"{}\"", cuerpo.replace('"', "\"\""))
    } else {
        cuerpo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(seccion: &str, campos: &[&str]) -> String {
        format!("{MARCA}{seccion}{US}{}", campos.join(&US.to_string()))
    }

    #[test]
    fn una_seccion_rota_no_se_lleva_las_otras_cuatro() {
        // `Get-ScheduledTask` necesita permisos que `Get-Service` no. En un
        // equipo donde Lucy entra sin ser administrador fallan las tareas y
        // funciona todo lo demás; un error global tiraría las cuatro listas
        // buenas por culpa de la quinta.
        let salida = [
            l("ports", &["443", "nginx"]),
            l("services", &["sshd", "running", "OpenSSH"]),
            l("software", &["git", "2.45.1"]),
            l("certs", &["/etc/x.pem", "CN=api", "1786060800"]),
            format!("{MARCA}err{US}tasks{US}Acceso denegado"),
        ]
        .join("\n");
        let inv = parse(&salida);
        assert_eq!(inv.ports.len(), 1);
        assert_eq!(inv.services.len(), 1);
        assert_eq!(inv.software.len(), 1);
        assert_eq!(inv.certs.len(), 1);
        assert!(inv.tasks.is_empty());
        assert_eq!(inv.fallo_de(Categoria::Tareas), Some("Acceso denegado"));
        // Y las que fueron bien no aparecen como fallidas.
        assert_eq!(inv.fallo_de(Categoria::Puertos), None);
    }

    #[test]
    fn una_tarea_de_cron_con_barra_vertical_llega_entera() {
        // EL CASO QUE ELIGIÓ EL SEPARADOR. Con `|` esta línea se parte en campos
        // fantasma y la tarea sale cortada justo por la mitad que dice qué hace.
        let cron = "0 3 * * * /bin/backup | mail -s ok root";
        let inv = parse(&l("tasks", &[cron]));
        assert_eq!(inv.tasks[0].entry, cron);
    }

    #[test]
    fn las_comillas_y_las_barras_llegan_sin_mutilar() {
        // La V2 BORRA caracteres del dato para que su JSON casero no reviente:
        // `tr -d '"\'` sobre el asunto del certificado. Aquí no hay nada que
        // escapar, así que no hay nada que borrar.
        let inv = parse(&[
            l("certs", &["C:\\certs\\a.pem", "CN=\"Acme, Inc.\", O=Acme", "1786060800"]),
            l("software", &["C:\\Program Files\\Node.js", "20.14.0"]),
        ]
        .join("\n"));
        assert_eq!(inv.certs[0].subject, "CN=\"Acme, Inc.\", O=Acme");
        assert_eq!(inv.certs[0].path, "C:\\certs\\a.pem");
        assert_eq!(inv.software[0].name, "C:\\Program Files\\Node.js");
    }

    #[test]
    fn lo_que_no_es_nuestro_no_entra_en_la_tabla() {
        // La salida trae avisos de PowerShell y errores de permisos de `find`, y
        // en WinRM las dos salidas llegan mezcladas. Sin la marca, «Permission
        // denied» entraría como si fuera un puerto.
        let salida = format!(
            "find: '/etc/ssl/private': Permission denied\n\
             AVISO: el módulo no está firmado\n\
             {}\n\
             \n\
             LUCY:ports\n\
             {}",
            l("ports", &["22", "sshd"]),
            l("ports", &["no-es-un-numero", "x"])
        );
        let inv = parse(&salida);
        assert_eq!(inv.ports.len(), 1, "{:?}", inv.ports);
        assert_eq!(inv.ports[0].port, 22);
    }

    #[test]
    fn un_recorte_dice_cuantos_habia() {
        // Una lista recortada en silencio se lee como una lista completa, y sobre
        // inventario eso hace concluir que un paquete no está instalado cuando lo
        // que pasa es que no cupo.
        let salida = (0..MAX_SOFTWARE + 37)
            .map(|i| l("software", &[&format!("paquete-{i}"), "1.0"]))
            .collect::<Vec<_>>()
            .join("\n");
        let inv = parse(&salida);
        assert_eq!(inv.software.len(), MAX_SOFTWARE);
        assert_eq!(inv.truncado, vec![(Categoria::Software, MAX_SOFTWARE + 37)]);
        // Lo que cabe no se marca.
        let corto = parse(&l("software", &["git", "2.45"]));
        assert!(corto.truncado.is_empty());
    }

    #[test]
    fn un_certificado_caducado_da_negativo_y_no_cero() {
        // `-1 / 86400` en Rust da 0, así que uno que caducó hace una hora saldría
        // como «le quedan 0 días» — indistinguible de uno que caduca esta noche.
        let ahora = 1_786_060_800_i64;
        let c =
            |ep: Option<i64>| Cert { path: String::new(), subject: String::new(), expires_epoch: ep };
        assert_eq!(c(Some(ahora + 10 * 86_400)).days_left(ahora), Some(10));
        assert_eq!(c(Some(ahora - 3600)).days_left(ahora), Some(-1), "un caducado no puede dar 0");
        assert_eq!(c(Some(ahora - 10 * 86_400)).days_left(ahora), Some(-10));
        // Y el de hoy mismo sí es cero.
        assert_eq!(c(Some(ahora + 3600)).days_left(ahora), Some(0));
        // NO SABERLO NO ES 1970. En Alpine y en BSD, `date -d` no entiende el
        // formato de `openssl x509` y el campo vuelve vacío. Con un cero, la
        // vista pintaba en rojo «caducó hace 20672d» sobre un certificado que
        // podía estar impecable.
        assert_eq!(c(None).days_left(ahora), None);
        assert_eq!(parse(&l("certs", &["/a.pem", "CN=x", ""])).certs[0].expires_epoch, None);
        assert_eq!(
            parse(&l("certs", &["/a.pem", "CN=x", "no-numero"])).certs[0].expires_epoch,
            None
        );
    }

    #[test]
    fn el_script_de_windows_lleva_la_consulta_unica_de_software() {
        // Tenerla duplicada produjo, según el comentario de la V2, «un aviso
        // CRITICAL al lado de un panel de Inventario marcando 0».
        let s = windows_script();
        assert!(s.contains(INSTALLED_SOFTWARE_PS), "la consulta no es la compartida");
        // Y las dos ramas del registro: sin Wow6432Node desaparece todo el
        // software de 32 bits en un Windows de 64.
        assert!(INSTALLED_SOFTWARE_PS.contains("Wow6432Node"));
        // Cada sección con su try/catch: es lo que permite que una falle sola.
        assert_eq!(s.matches("catch").count(), 5, "{s}");
        // Y SIN RECORTAR LISTAS EN EL EQUIPO. `-First 1` sí está y es otra cosa
        // —elige un elemento del grupo de un puerto—; lo que no puede haber es un
        // tope sobre el resultado, que llega corto y parece completo. El de la V2
        // era `-First 250` sobre el software, justo la lista que alimenta el
        // cruce de vulnerabilidades.
        for tope in ["-First 250", "-First 50", "-First 20", "-First 60"] {
            assert!(!s.contains(tope), "recorta en el equipo remoto: {tope}");
        }
    }

    #[test]
    fn los_scripts_y_el_analizador_llaman_igual_a_las_secciones() {
        // EL FALLO QUE ESTE TEST EXISTE PARA COGER: renombrar una sección en el
        // script y no en `de_clave`. No da error en ninguna parte — las líneas de
        // esa sección dejan de reconocerse y la categoría sale vacía, que se lee
        // como «este equipo no tiene tareas programadas».
        let win = windows_script();
        let lin = linux_script();
        for c in Categoria::ALL {
            // Windows monta la marca en tiempo de ejecución con su función `W`,
            // así que lo que hay en el texto es el argumento entrecomillado.
            let arg = format!("'{}'", c.clave());
            assert!(win.contains(&arg), "windows no emite {}: {arg}", c.label());
            // Linux la escribe entera en cada `printf`/`awk`.
            let marca = format!("{MARCA}{}", c.clave());
            assert!(lin.contains(&marca), "linux no emite {}: {marca}", c.label());
            // Y la vuelta: lo que emite el script, el analizador lo reconoce.
            assert_eq!(Categoria::de_clave(c.clave()), Some(c));
        }
        // La sección de errores usa la MISMA clave que la de datos, o un fallo de
        // «tasks» se anotaría en una categoría que no existe y se perdería.
        for c in Categoria::ALL {
            let err_win = format!("E '{}'", c.clave());
            let err_lin = format!("{MARCA}err");
            assert!(
                win.contains(&err_win),
                "windows no sabe informar de un fallo en {}",
                c.label()
            );
            assert!(lin.contains(&err_lin));
        }
    }

    #[test]
    fn el_script_de_linux_no_fabrica_json_ni_recorta_por_bytes() {
        // `head -c 8000` corta por BYTES en mitad de un registro, y como el
        // documento se deserializaba de una pieza se caía el inventario ENTERO
        // —puertos incluidos— en cualquier Debian con más de 180 paquetes.
        let s = linux_script();
        assert!(!s.contains("head -c"), "sigue recortando por bytes");
        assert!(!s.contains("tr -d"), "sigue borrando caracteres del dato");
        // Y cada sección dice si no puede consultarse, en vez de callar.
        assert_eq!(s.matches(&format!("{MARCA}err")).count(), 3, "{s}");
    }

    #[test]
    fn un_equipo_que_no_es_shell_no_se_inventaria() {
        let h = crate::hosts::Host {
            id: "h1".into(),
            name: "CACHE".into(),
            os: "linux".into(),
            protocol: crate::hosts::Protocol::Redis,
            host: "10.0.0.9".into(),
            username: String::new(),
            port: 6379,
            ssh_key_path: String::new(),
            tags: Vec::new(),
            color: "#fff".into(),
            category: crate::hosts::Category::Database,
            db_type: None,
        };
        let e = remote_script(&h).unwrap_err();
        assert!(e.contains("WinRM o SSH"), "{e}");
        assert!(e.contains("CACHE"), "no dice de qué equipo habla: {e}");
    }

    #[test]
    fn el_csv_entrecomilla_un_asunto_con_comas() {
        // El asunto de un certificado lleva comas casi siempre
        // —`CN=api, O=Acme, C=ES`— así que sin esto el fichero sale con las
        // columnas desplazadas justo en la tabla que más se exporta.
        let mut inv = Inventory::default();
        inv.certs.push(Cert {
            path: "/etc/x.pem".into(),
            subject: "CN=api, O=Acme \"IT\", C=ES".into(),
            expires_epoch: Some(1_786_060_800),
        });
        let c = to_csv(&inv, "WIN-AD");
        let fila = c.lines().nth(1).unwrap();
        assert!(fila.contains("\"CN=api, O=Acme \"\"IT\"\", C=ES\""), "{fila}");
        assert_eq!(fila.split(',').count(), 7, "columnas desplazadas: {fila}");
    }

    #[test]
    fn los_caracteres_de_control_se_neutralizan_sin_pegar_los_campos() {
        // BORRARLOS ERA EL FALLO. `/etc/cron.d/*` separa sus campos con
        // TABULADOR, y el tabulador es Cc igual que la campana: el filtro se los
        // llevaba a los dos por igual y la tarea salía como
        // `30 4 * * 1root/usr/local/bin/backup.sh`, ilegible e imposible de
        // buscar.
        let cron = "30 4 * * 1\troot\t/usr/local/bin/backup.sh --full";
        let inv = parse(&l("tasks", &[cron]));
        assert_eq!(inv.tasks[0].entry, "30 4 * * 1 root /usr/local/bin/backup.sh --full");

        // Y lo que motivó el filtro sigue cubierto: una campana no suena al
        // copiar la tabla y una secuencia de escape no llega al portapapeles.
        // El precio es que parten la palabra en dos, y es el precio correcto:
        // una campana dentro de un nombre no es un nombre real, y una línea de
        // cron sí es una línea de cron.
        let s = parse(&l("services", &["ssh\u{7}d", "running", "Open\u{1b}[31mSSH"]));
        assert!(!s.services[0].name.contains('\u{7}'));
        assert_eq!(s.services[0].name, "ssh d");
        assert_eq!(s.services[0].description, "Open [31mSSH");
    }

    #[test]
    fn una_sesion_cortada_a_medias_no_pasa_por_una_foto_completa() {
        // La sesión se cae después de los puertos: llegan cuarenta líneas buenas
        // y ninguna marca de error. Mirando solo «¿hay datos?», eso pasaba por un
        // éxito y el operador leía «Servicios (0)» como un hecho del servidor.
        let inv = parse(&l("ports", &["443", "nginx"]));
        let cortado = cierra(inv.clone(), "Connection closed by remote host", false, "SRV")
            .expect("hay datos, no puede ser un error entero");
        assert!(cortado.parcial.is_some(), "no avisa de que puede faltar información");
        assert!(cortado.parcial.unwrap().contains("Connection closed"));
        assert_eq!(cortado.ports.len(), 1, "y no se pierde lo que sí llegó");

        // Una foto entera no lleva la marca.
        let entero = cierra(inv, "", true, "SRV").unwrap();
        assert!(entero.parcial.is_none());

        // Y sin nada de nada sigue siendo un error, no una foto vacía.
        assert!(cierra(Inventory::default(), "", true, "SRV").is_err());
    }

    #[test]
    fn el_csv_desactiva_lo_que_excel_leeria_como_formula() {
        // `@reboot /usr/local/bin/sync.sh` es una tarea de cron normal, y Excel y
        // LibreOffice tratan `@`, `=`, `+` y `-` como comienzo de fórmula al
        // importar: la celda muestra `#NAME?` en vez de la tarea. Entrecomillar
        // no lo desactiva — es cosa del importador, no del CSV.
        let mut inv = Inventory::default();
        inv.tasks.push(Task {
            entry: "@reboot /usr/local/bin/sync.sh".into(),
            state: String::new(),
        });
        let c = to_csv(&inv, "SRV");
        assert!(c.contains("\"'@reboot /usr/local/bin/sync.sh\""), "{c}");
        // Y un valor normal no se toca.
        assert!(to_csv(&Inventory::default(), "SRV").starts_with("equipo,"));
    }
}
