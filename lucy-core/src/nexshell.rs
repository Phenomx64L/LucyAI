//! Lo que distingue a NexShell de una consola cualquiera.
//!
//! La idea del módulo es que escribes en el mismo campo un COMANDO o una FRASE,
//! y él decide. `Get-Service` se ejecuta; «¿por qué va lento el disco?» se
//! traduce a un comando, se comprueba, y entonces se ejecuta. Sin dos campos,
//! sin un botón de modo, sin acordarse de nada.
//!
//! Aquí vive esa decisión y la limpieza de lo que devuelve el modelo. El PTY, la
//! pantalla y los botones son de la interfaz; esto es lo que se puede probar sin
//! abrir una ventana, y por eso está separado.

/// Los verbos que se reconocen como comando aunque vayan seguidos de palabras.
///
/// La misma lista que la V2, y mezcla los dos mundos —`ls` y `dir`, `ps` y
/// `tasklist`— porque un mismo NexShell habla con Windows y con Linux según el
/// host. Un verbo de más aquí solo hace que una frase rara se intente ejecutar y
/// falle con un error del shell; uno de menos manda a traducir algo que el
/// operador escribió literalmente para que se ejecutara, y eso cuesta una
/// llamada al modelo y una sorpresa.
const KNOWN: &[&str] = &[
    "ls", "cd", "cat", "pwd", "echo", "df", "du", "ps", "top", "htop", "free", "uname", "whoami",
    "id", "uptime", "date", "systemctl", "service", "journalctl", "dnf", "yum", "apt", "apt-get",
    "pacman", "zypper", "rpm", "dpkg", "snap", "docker", "podman", "kubectl", "git", "grep",
    "egrep", "awk", "sed", "find", "tail", "head", "less", "more", "ss", "netstat", "ip",
    "ifconfig", "ping", "curl", "wget", "ssh", "scp", "rsync", "tar", "gzip", "gunzip", "zip",
    "unzip", "chmod", "chown", "mkdir", "rmdir", "rm", "cp", "mv", "ln", "touch", "kill",
    "killall", "pkill", "nohup", "crontab", "mount", "umount", "lsblk", "fdisk", "mkfs", "sudo",
    "su", "env", "export", "set", "history", "man", "which", "whereis", "type", "sh", "bash",
    "zsh", "python", "python3", "node", "npm", "pip", "pip3", "make", "gcc", "vim", "nano",
    "reboot", "shutdown", "poweroff", "halt", "hostnamectl", "timedatectl", "firewall-cmd",
    "iptables", "nft", "getenforce", "dir", "ipconfig", "tasklist", "taskkill", "sc", "net",
    "reg", "wmic", "powershell", "pwsh",
];

/// Si lo escrito parece un comando y no una petición en lenguaje natural.
///
/// EL ERROR CARO ES EL FALSO NEGATIVO. Tomar un comando por una frase lo manda a
/// traducir: se paga una llamada al modelo, se tarda, y lo que vuelve puede no
/// ser lo que el operador escribió con sus manos. Tomar una frase por un comando
/// solo produce un error del shell, que se lee y se reescribe. Por eso las
/// reglas se inclinan hacia «es un comando».
pub fn looks_like_command(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    // El primer token, cortado en el primer separador: `ps|grep x` empieza por
    // `ps`, y sin cortar sería un verbo desconocido.
    let primero = t
        .split_whitespace()
        .next()
        .unwrap_or("")
        .split(['|', ';', '&'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    if KNOWN.contains(&primero.as_str()) {
        return true;
    }
    // Un verbo de PowerShell es `Verbo-Sustantivo`, y son cientos: enumerarlos
    // sería una lista imposible de mantener. La FORMA los reconoce a todos.
    // Esto no está en la V2 y aquí hace falta: el shell local es PowerShell, y
    // sin ello `Get-Service` se mandaba a traducir.
    if es_verbo_powershell(&primero) {
        return true;
    }
    // Una ruta, una tubería, una redirección, una sustitución o una asignación.
    if t.starts_with(['.', '~', '/', '\\'])
        || t.contains('|')
        || t.contains('<')
        || t.contains('>')
        || t.contains("&&")
        || t.contains("$(")
    {
        return true;
    }
    // `algo -x` o `algo --largo`: la forma de una opción.
    if let Some(resto) = t.split_once(char::is_whitespace) {
        let seg = resto.1.trim_start();
        if seg.starts_with('-') && seg.len() > 1 && seg[1..].starts_with(|c: char| c.is_alphanumeric() || c == '-')
        {
            return true;
        }
    }
    // `VAR=valor`.
    if let Some((izq, _)) = t.split_once('=') {
        if !izq.is_empty()
            && !izq.contains(char::is_whitespace)
            && izq.starts_with(|c: char| c.is_alphabetic() || c == '_')
        {
            return true;
        }
    }
    // Un solo token sin espacios: casi siempre un programa.
    !t.contains(char::is_whitespace)
}

/// `Get-Service`, `Set-ItemProperty`, `Remove-Item`… la forma canónica de un
/// cmdlet: dos palabras alfabéticas unidas por UN guion.
///
/// Se exige que las dos partes sean letras para no tragarse `apt-get` (que ya
/// está en la lista) ni `--algo`.
fn es_verbo_powershell(tok: &str) -> bool {
    let Some((verbo, sustantivo)) = tok.split_once('-') else {
        return false;
    };
    !verbo.is_empty()
        && !sustantivo.is_empty()
        && verbo.chars().all(|c| c.is_ascii_alphabetic())
        && sustantivo.chars().all(|c| c.is_ascii_alphabetic())
}

/// Limpia lo que devuelve el modelo hasta dejar UN comando.
///
/// Un modelo al que se le pide «solo el comando» devuelve el comando dentro de
/// un bloque de código la mitad de las veces, y con un `$` delante la otra
/// mitad. Ejecutar eso tal cual da un error que no se parece en nada a la causa.
pub fn clean_command(raw: &str) -> String {
    let mut c = raw.trim().to_string();
    // Vallas de bloque de código, con o sin lenguaje.
    if let Some(resto) = c.strip_prefix("```") {
        c = resto.split_once('\n').map(|x| x.1).unwrap_or("").to_string();
    }
    if let Some(i) = c.rfind("```") {
        c.truncate(i);
    }
    let c = c.trim().trim_matches('`').trim();
    // La PRIMERA línea con contenido. Si el modelo añadió una explicación
    // debajo, la explicación no es el comando.
    let primera = c.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    // El `$` o el `>` del ejemplo de un manual.
    primera
        .trim_start_matches(['$', '>'])
        .trim()
        .to_string()
}

/// Lo que se le pide al modelo para traducir una frase a un comando.
///
/// UNA SOLA LÍNEA Y SIN EXPLICACIÓN, dicho tres veces de tres formas, porque es
/// lo que un modelo servicial quiere desobedecer. Y con la distro delante: sin
/// ella propone `apt` en una Fedora, que es un error que se ve tarde.
pub fn translate_prompt(peticion: &str, shell: &str, so: &str) -> String {
    format!(
        "Traduce la siguiente petición a UN SOLO comando de {shell} para un equipo que \
         ejecuta: {so}. Petición: \"{peticion}\".\n\
         Reglas: responde SOLO con el comando, sin explicación, sin markdown, sin comillas \
         envolventes. Prefiere comandos NO interactivos. Usa el gestor de paquetes que \
         corresponda a ese sistema (dnf en Fedora/RHEL, apt en Debian/Ubuntu, pacman en Arch, \
         zypper en SUSE, apk en Alpine; winget o Get-WindowsUpdate en Windows).\n\
         Si no se puede resolver en un comando, responde exactamente: SIN_COMANDO"
    )
}

/// Lo que el modelo contesta cuando no hay comando posible.
pub const NO_COMMAND: &str = "SIN_COMANDO";

/// Cuanta salida del comando fallido se le enseña al modelo.
///
/// Mil doscientos caracteres. La causa de un fallo esta casi siempre en las
/// primeras lineas del error —«no se reconoce como cmdlet», «permiso denegado»,
/// «no such file»— y mandar la salida entera de un `Get-EventLog` que ademas
/// fallo al final serian decenas de miles de tokens para diagnosticar algo que
/// cabia en tres renglones.
pub const MAX_SALIDA_DIAG: usize = 1_200;

/// Lo que se le pide al modelo cuando un comando ha fallado.
///
/// ── POR QUE ESTO NO ES OTRA TRADUCCION ───────────────────────────────────────
///
/// `translate_prompt` pide UN COMANDO y prohibe explicar. Aqui es al reves: lo
/// que hace falta primero es la CAUSA en una linea, porque un comando alternativo
/// sin saber por que fallo el primero es adivinar delante del operador.
///
/// El formato de dos campos es el mismo truco que en `suggest`: plano, con una
/// marca por linea, y lo que no encaje se descarta sin drama. Un modelo pequeño
/// devuelve JSON roto la mitad de las veces.
///
/// Y EL COMANDO ES OPCIONAL A PROPOSITO. Hay fallos que no se arreglan con otro
/// comando —una credencial que caduco, un servicio que hay que levantar a mano—
/// y ofrecer uno cualquiera para no dejar el hueco vacio es peor que decir que no
/// lo hay.
pub fn diagnose_prompt(cmd: &str, salida: &str, shell: &str, so: &str) -> String {
    let corta: String = salida.chars().take(MAX_SALIDA_DIAG).collect();
    format!(
        "Un comando de {shell} ha fallado en un equipo que ejecuta: {so}.

         Comando: {cmd}
         Salida:
{corta}

         Contesta EXACTAMENTE en este formato, sin markdown y sin nada mas:
         CAUSA: <por que fallo, en una sola frase corta>
         PRUEBA: <un solo comando de {shell} que lo resuelva o avance>

         Si no hay ningun comando que lo arregle, escribe la linea PRUEBA asi:
         PRUEBA: {NO_COMMAND}"
    )
}

/// Lo que el modelo contesto al diagnosticar: la causa y, si la hay, otra vía.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diagnostico {
    pub causa: String,
    /// Vacio = no hay comando que lo arregle, y decirlo es una respuesta.
    pub prueba: String,
}

/// Lee la respuesta del diagnostico.
///
/// TOLERANTE CON EL ADORNO, igual que el lector de atajos: viñetas, asteriscos y
/// dos puntos de mas. Lo que no se tolera es inventar — si no hay linea de causa,
/// no hay diagnostico, y quien llama se calla en vez de enseñar medio.
pub fn parse_diagnostico(salida: &str) -> Option<Diagnostico> {
    // Los razonadores abren `<think>`: dentro hay frases que empiezan por CAUSA.
    let limpio = match salida.rfind("</think>") {
        Some(i) => &salida[i + "</think>".len()..],
        None => salida,
    };
    let mut d = Diagnostico::default();
    for linea in limpio.lines() {
        let l = linea.trim().trim_start_matches(['-', '*', '·', '•', ' ', '\t', '#']);
        let bajo = l.to_ascii_lowercase();
        // EL ESPACIO VA EN EL MISMO CONJUNTO QUE LOS ADORNOS, no en un `trim`
        // aparte. Es la lección que `suggest::parse` ya lleva escrita: con
        // `**CAUSA:** "no existe la ruta"`, quitar primero los asteriscos deja
        // el recorte parado en el espacio de detrás, y sale ` "no existe la
        // ruta` con la comilla pegada. Un solo recorte que se coma las dos
        // cosas lo resuelve.
        let corta = |l: &str, n: usize| {
            l[n..]
                .trim_matches(|c: char| {
                    c.is_whitespace() || matches!(c, '"' | '«' | '»' | '*' | ':')
                })
                .to_string()
        };
        if d.causa.is_empty() {
            if let Some(p) = bajo.find("causa:") {
                d.causa = corta(l, p + "causa:".len());
                continue;
            }
        }
        if d.prueba.is_empty() {
            if let Some(p) = bajo.find("prueba:") {
                d.prueba = clean_command(&corta(l, p + "prueba:".len()));
            }
        }
    }
    if d.causa.is_empty() {
        return None;
    }
    // «SIN_COMANDO» es una respuesta, no un comando. Se convierte en el hueco
    // vacio que ya significa «no hay otra via».
    if d.prueba.contains(NO_COMMAND) {
        d.prueba.clear();
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_cmdlet_de_powershell_no_se_manda_a_traducir() {
        // Esto NO está en la V2 y aquí hace falta: el shell local es PowerShell.
        // Sin reconocer la forma `Verbo-Sustantivo`, escribir `Get-Service` a
        // mano pagaba una llamada al modelo para que devolviera... `Get-Service`.
        for c in [
            "Get-Service",
            "Get-Service | Where-Object Status -eq 'Stopped'",
            "Remove-Item C:\\temp -Recurse",
            "Set-ItemProperty -Path HKLM:\\x -Name y -Value 1",
        ] {
            assert!(looks_like_command(c), "lo mandó a traducir: {c}");
        }
    }

    #[test]
    fn una_frase_se_manda_a_traducir() {
        for s in [
            "¿por qué va lento el disco?",
            "dime qué servicios están caídos",
            "reinicia el servicio de impresión por favor",
            "cuánta memoria queda libre",
        ] {
            assert!(!looks_like_command(s), "lo tomó por comando: {s}");
        }
    }

    #[test]
    fn los_verbos_conocidos_valen_con_argumentos_detras() {
        assert!(looks_like_command("systemctl status nginx"));
        assert!(looks_like_command("ps|grep node"), "no cortó por la tubería");
        assert!(looks_like_command("tasklist /svc"));
        assert!(looks_like_command("dir"));
    }

    #[test]
    fn la_forma_delata_al_comando_aunque_el_verbo_no_se_conozca() {
        assert!(looks_like_command("./script.ps1"), "ruta relativa");
        assert!(looks_like_command("C:\\ops\\hazlo.cmd"), "ruta absoluta");
        assert!(looks_like_command("miprograma --verbose"), "opción larga");
        assert!(looks_like_command("LC_ALL=C algo"), "asignación");
        assert!(looks_like_command("algoquenoconozco"), "un solo token");
    }

    #[test]
    fn ante_la_duda_es_un_comando() {
        // El falso negativo es el caro: manda a traducir algo que el operador
        // escribió con sus manos, y lo que vuelve puede no ser lo que escribió.
        // El falso positivo solo da un error del shell, que se lee y se corrige.
        assert!(looks_like_command("nosequeesesto"));
        assert!(!looks_like_command(""));
        assert!(!looks_like_command("   "));
    }

    #[test]
    fn el_comando_se_saca_de_donde_lo_meta_el_modelo() {
        // Las tres formas en que devuelve un comando cuando se le pide que no
        // lo adorne. Ejecutarlas tal cual da un error que no se parece a la
        // causa.
        assert_eq!(clean_command("```powershell\nGet-Service\n```"), "Get-Service");
        assert_eq!(clean_command("`Get-Service`"), "Get-Service");
        assert_eq!(clean_command("$ Get-Service"), "Get-Service");
        assert_eq!(clean_command("  Get-Service  "), "Get-Service");
    }

    #[test]
    fn una_explicacion_detras_no_es_parte_del_comando() {
        // Se queda la primera línea con contenido. Sin esto, la explicación
        // acabaría en la línea de comandos.
        let r = clean_command("Get-Volume\n\nEsto lista los volúmenes del equipo.");
        assert_eq!(r, "Get-Volume");
    }

    #[test]
    fn se_le_dice_al_modelo_en_que_sistema_esta() {
        // Sin la distro delante propone `apt` en una Fedora, y ese error se ve
        // tarde: el comando existe, simplemente no está.
        let p = translate_prompt("instala nginx", "bash", "Fedora 41");
        assert!(p.contains("Fedora 41"));
        assert!(p.contains("bash"));
        assert!(p.contains("dnf"), "no le recuerda el gestor de paquetes");
        assert!(p.contains(NO_COMMAND), "no le da una salida para lo imposible");
    }

    #[test]
    fn el_diagnostico_saca_la_causa_y_la_alternativa() {
        // Exactamente el fallo de la captura del operador: `Get-WindowsUpdate`
        // sobre un Server 2022 donde el modulo PSWindowsUpdate no esta puesto.
        let d = parse_diagnostico(
            "CAUSA: el modulo PSWindowsUpdate no esta instalado en ese servidor
             PRUEBA: Install-Module PSWindowsUpdate -Force -Scope AllUsers",
        )
        .expect("no leyo el diagnostico");
        assert!(d.causa.starts_with("el modulo PSWindowsUpdate"));
        assert_eq!(d.prueba, "Install-Module PSWindowsUpdate -Force -Scope AllUsers");
    }

    #[test]
    fn sin_alternativa_el_hueco_se_queda_vacio() {
        // Hay fallos que no arregla otro comando —una credencial caducada, un
        // servicio que hay que levantar a mano— y ofrecer uno cualquiera para no
        // dejar el hueco vacio es peor que decir que no lo hay.
        let d = parse_diagnostico(
            "CAUSA: la credencial guardada ya no vale
PRUEBA: SIN_COMANDO",
        )
        .expect("no leyo la causa");
        assert!(!d.causa.is_empty());
        assert!(d.prueba.is_empty(), "invento un comando: {:?}", d.prueba);
    }

    #[test]
    fn sin_causa_no_hay_diagnostico() {
        // Medio diagnostico es peor que ninguno: un comando propuesto sin decir
        // por que fallo el anterior es adivinar delante del operador.
        assert!(parse_diagnostico("PRUEBA: Get-Service").is_none());
        assert!(parse_diagnostico("").is_none());
        assert!(parse_diagnostico("no se que decirte").is_none());
    }

    #[test]
    fn aguanta_que_el_modelo_lo_adorne() {
        // Viñetas, asteriscos y comillas: lo mismo que ya tolera el lector de
        // atajos, y por la misma razon.
        let d = parse_diagnostico(
            "- **CAUSA:** \"no existe la ruta\"\n  \
             * PRUEBA: `New-Item -ItemType Directory /opt/lucy`",
        )
        .expect("el adorno lo tumbo");
        assert_eq!(d.causa, "no existe la ruta");
        assert_eq!(d.prueba, "New-Item -ItemType Directory /opt/lucy");
    }

    #[test]
    fn el_bloque_de_pensamiento_no_se_cuela() {
        // Un razonador escribe dentro de `<think>` frases que empiezan por
        // CAUSA. La de dentro no es la respuesta.
        let d = parse_diagnostico(
            "<think>CAUSA: quiza sea el permiso</think>
CAUSA: falta el modulo
PRUEBA: SIN_COMANDO",
        )
        .expect("no leyo");
        assert_eq!(d.causa, "falta el modulo");
    }

    #[test]
    fn la_salida_que_se_manda_esta_acotada() {
        // Un `Get-EventLog` que falla al final trae decenas de miles de
        // caracteres, y la causa esta en los primeros renglones.
        let enorme = "x".repeat(50_000);
        let p = diagnose_prompt("Get-EventLog -LogName X", &enorme, "PowerShell", "Windows Server 2022");
        assert!(p.len() < MAX_SALIDA_DIAG + 1_000, "manda la salida entera: {} chars", p.len());
        assert!(p.contains("Get-EventLog"), "perdio el comando que fallo");
        assert!(p.contains("Windows Server 2022"), "perdio el sistema del equipo");
        assert!(p.contains(NO_COMMAND), "no le da salida para lo que no se arregla");
    }
}
