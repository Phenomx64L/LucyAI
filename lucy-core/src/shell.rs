//! Ejecución de PowerShell y decodificación de lo que devuelve.
//!
//! Cuarto lote hacia el corazón compartido, y el primero que no desbloquea una
//! vista sino tres: Inventario, Compliance y buena parte de NexShell hacen lo
//! mismo —lanzar PowerShell y leer su salida— y ninguna puede migrar sin esto.
//!
//! Lo que se queda en `src-tauri/src/utils/shell.rs` es lo que no es genérico:
//! WinRM, la validación de hosts y los guardrails sobre credenciales. Eso es
//! política de un anfitrión que expone comandos a un LLM, igual que la guarda de
//! rutas del lote anterior.

/// Lanza el proceso sin abrir una ventana de consola. Lucy es una app gráfica:
/// sin esto, cada comando parpadea una ventana negra en la cara del usuario.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ── Decodificar lo que escriben las herramientas nativas ─────────────────────
//
// A PowerShell se le PUEDE pedir que escriba UTF-8 (ver el preámbulo abajo).
// A `tasklist`, `netstat`, `reg` o `ipconfig` no: escriben la página de códigos
// OEM del sistema —CP-850 en una instalación en español, CP-437 en una en
// inglés— y no hay interruptor.
//
// Medido en esta máquina con la consola ya en UTF-8: `tasklist` y `systeminfo`
// devuelven UTF-8 válido, `netstat -an` e `ipconfig /all` no. Es por herramienta,
// no por máquina. Leídas todas como UTF-8, un adaptador llamado
// "Descripción" llegaba como "Descripci<?>n" — y esa salida se le entrega al
// modelo como verdad sobre el equipo, así que una ruta con un carácter de
// reemplazo es una ruta contra la que después propone comandos.
//
// `encoding_rs` no sirve: implementa el juego WHATWG, que cubre windows-1252 y
// no CP-850. La página OEM es un concepto de Windows y solo Windows sabe cuál
// es, de ahí `MultiByteToWideChar(CP_OEMCP)`.

/// Decodifica bytes escritos por una herramienta nativa de consola.
///
/// UTF-8 primero, y gana cuando es válido. El orden es deliberado: ASCII puro
/// —la inmensa mayoría de esta salida— es UTF-8 válido y pasa intacto, y las
/// herramientas que sí emiten UTF-8 siguen funcionando. Solo los bytes que NO
/// son UTF-8 válido, que es a lo que se parece el texto OEM con acentos, van al
/// decodificador OEM.
///
/// Una secuencia OEM corta puede ser además UTF-8 válido y leerse como los
/// caracteres equivocados. Ese residuo es inevitable sin conocer la codificación
/// de cada herramienta, y sustituye a una ruta que salía mal SIEMPRE.
pub fn decode_console(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => decode_oem(bytes),
    }
}

#[cfg(windows)]
fn decode_oem(bytes: &[u8]) -> String {
    use winapi::um::stringapiset::MultiByteToWideChar;
    use winapi::um::winnls::CP_OEMCP;

    if bytes.is_empty() {
        return String::new();
    }
    // `i32` porque es lo que toma la API. Una herramienta que produjera 2 GB
    // tiene otro problema, y el respaldo lossy es una respuesta correcta para
    // ella en vez de un pánico.
    let Ok(len) = i32::try_from(bytes.len()) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };

    unsafe {
        // Primera llamada para dimensionar, segunda para llenar.
        let needed = MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr() as *const i8,
            len,
            std::ptr::null_mut(),
            0,
        );
        if needed <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide: Vec<u16> = vec![0; needed as usize];
        let written = MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr() as *const i8,
            len,
            wide.as_mut_ptr(),
            needed,
        );
        if written <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        wide.truncate(written as usize);
        String::from_utf16_lossy(&wide)
    }
}

#[cfg(not(windows))]
fn decode_oem(bytes: &[u8]) -> String {
    // Fuera de Windows no hay página OEM. Presente para que el módulo compile
    // en un `cargo check` de otra plataforma.
    String::from_utf8_lossy(bytes).into_owned()
}

// ── PowerShell, decodificado bien ────────────────────────────────────────────

/// Preámbulo que hace que un PowerShell lanzado escriba UTF-8 a la tubería.
///
/// Lucy es un proceso gráfico sin consola, así que un PowerShell hijo escribe en
/// la página OEM del sistema — donde `ó` es el byte suelto `0xA2`. Eso no es
/// UTF-8 válido y `from_utf8_lossy` lo sustituye por U+FFFD: el texto llega roto
/// sin que nada dé error.
///
/// El ORDEN importa y no es cosmético: PowerShell fija la codificación de un
/// flujo en la primera escritura, así que esto tiene que ir ANTES del script.
/// Y hacen falta los dos manejadores: `[Console]::OutputEncoding` cubre lo que
/// el proceso escribe a la tubería, `$OutputEncoding` cubre lo que le pasa a un
/// comando nativo en una tubería interna.
pub const PS_UTF8_PREAMBLE: &str =
    "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()\n\
     $OutputEncoding = [System.Text.UTF8Encoding]::new()\n";

/// El marcador con el que el epílogo cuenta lo que no se ve desde fuera.
const FIN: &str = "##LUCY-FIN ";

/// Lo que se añade DETRÁS del script para poder saber si de verdad fue bien.
///
/// ── EL FALLO QUE ESTO CIERRA, Y ESTÁ MEDIDO ─────────────────────────────────
///
/// El veredicto era `out.status.success()` a secas. Y en PowerShell un error NO
/// TERMINANTE —que es el modo por defecto de casi todos los cmdlets— se escribe,
/// se ve, y el proceso sale con CERO:
///
/// ```text
///   powershell -Command "Get-Item 'C:\no-existe'; Get-Date"   → salida 0
/// ```
///
/// O sea que Lucy lo apuntaba como éxito. El paso del plan iba a «hecho», la fila
/// de auditoría guardaba `exit_code` 0, `fallos_recientes` no lo contaba nunca, y
/// al modelo se le devolvía la salida con el mismo remate que si hubiera
/// funcionado — así que proponía lo siguiente sobre algo que no pasó.
///
/// ── POR QUÉ `$Error.Count` Y NO MIRAR stderr A SECAS ────────────────────────
///
/// Porque stderr solo no distingue lo que hay que distinguir. Medido:
///
/// ```text
///                                                stderr   $Error   veredicto
///   Write-Warning / Verbose / Host / Information      0        0   bien
///   error silenciado con -ErrorAction Silently…       0        1   bien
///   programa NATIVO que escribe en stderr y va bien  21        0   bien
///   error de PowerShell mostrado                    439        1   FALLO
/// ```
///
/// Ni los avisos ni `Write-Host` llegan a stderr, así que no ensucian. Pero un
/// programa nativo SÍ escribe ahí yendo bien —`git`, `winget`— y un error
/// silenciado a propósito sube `$Error` sin escribir nada. Solo las DOS cosas
/// juntas significan «hubo un error y se vio».
///
/// ── POR QUÉ REPONE `$LASTEXITCODE`, Y LO ENCONTRÓ UNA PRUEBA ────────────────
///
/// Porque el epílogo pasa a ser la ÚLTIMA instrucción del script, y PowerShell
/// saca su código de salida de cómo acabó la última. Sin reponerlo, un
/// `cmd /c exit 3` —que antes salía con 3— pasaba a salir con 0: el arreglo
/// rompía justo el caso que ya funcionaba.
///
/// Las seis pruebas del analizador pasaban igual de verdes. Lo cazó la que lanza
/// PowerShell de verdad, y por eso esa prueba existe.
///
/// ── SI EL SCRIPT LLAMA A `exit`, ESTO NO CORRE ──────────────────────────────
///
/// Y entonces no hay marcador. Ese caso vuelve al veredicto de antes, que para un
/// `exit` explícito es exactamente el correcto: el código que puso el script.
const PS_EPILOGO: &str = "\n$__lucy_code = $LASTEXITCODE\n\
     [Console]::Error.WriteLine(\"##LUCY-FIN {0}\" -f $Error.Count)\n\
     if ($__lucy_code) { exit $__lucy_code }\n";

/// Antepone el preámbulo a un script, y le pone el epílogo detrás.
pub fn ps_utf8(script: &str) -> String {
    format!("{PS_UTF8_PREAMBLE}{script}{PS_EPILOGO}")
}

/// Qué pasó de verdad: el canal de errores limpio, y si el comando funcionó.
///
/// Aparte y pura para poder fijar los cinco casos de la tabla de `PS_EPILOGO`
/// sin arrancar PowerShell. `salio_bien` es lo que decía el proceso — que sigue
/// mandando para lo nativo: un `cmd /c exit 3` al final SÍ sale con código, y ese
/// camino no cambia.
pub fn veredicto(stderr: &str, salio_bien: bool) -> (String, bool) {
    let Some(i) = stderr.rfind(FIN) else {
        // Sin marcador —el script llamó a `exit`, o PowerShell no llegó a
        // terminar— se cree lo que dijo el proceso, como antes.
        return (stderr.to_string(), salio_bien);
    };
    let errores: u32 = stderr[i + FIN.len()..]
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .parse()
        .unwrap_or(0);
    // El marcador se va: es fontanería de Lucy y no tiene nada que decirle al
    // operador ni al modelo.
    let limpio = format!("{}{}", &stderr[..i], stderr[i..].split_once('\n').map_or("", |(_, r)| r));
    let hubo_error_visible = errores > 0 && !limpio.trim().is_empty();
    (limpio.trim_end().to_string(), salio_bien && !hubo_error_visible)
}

/// Ejecuta un script de PowerShell y devuelve `(stdout, stderr, ok)`.
///
/// La salida pasa por `decode_console`, no por `from_utf8_lossy`: el preámbulo
/// cubre lo que PowerShell escribe por sí mismo, pero un script que invoca una
/// herramienta nativa —`netstat`, `reg`— trae los bytes de ésa mezclados.
#[cfg(windows)]
pub fn run_powershell_utf8(script: &str) -> Result<(String, String, bool), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    // SOLO SI SIGUE EXISTIENDO. `current_dir` con una carpeta que ya no está
    // hace fallar el `spawn` ENTERO, no el comando: PowerShell no llega a
    // arrancar y lo que vuelve es «The directory name is invalid», que no se
    // parece a lo que pasó. El operador elige una carpeta de un USB, la
    // desconecta, y de pronto Lucy no puede ejecutar NADA —ni un `Get-Date`— por
    // algo que no tiene que ver con el comando que pidió.
    let trabajo = crate::workdir::actual();
    let trabajo = if trabajo.is_dir() { Some(trabajo) } else { None };

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_utf8(script)]);
    // EN EL DIRECTORIO DE TRABAJO, y ésta es la fuga grande de las cuatro. Sin
    // esto el hijo hereda el directorio del proceso, que instalada es
    // `C:\Program Files\Lucy`. Todo comando que Lucy propone y ejecuta corría
    // ahí: un `New-Item informe.txt`, un `Export-Csv salida.csv`, un `>` a un
    // fichero sin ruta. Eso es literalmente lo que el operador describió como
    // «ciertos archivos se escriben en el proyecto» — lanzada con `cargo run`,
    // el directorio del proceso es el del repositorio.
    //
    // Ni siquiera daba error: la escritura funciona o muere con acceso denegado,
    // y en los dos casos el fichero no está donde se buscó.
    if let Some(d) = trabajo {
        cmd.current_dir(d);
    }

    let out = cmd
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell spawn failed: {}", e))?;
    // EL VEREDICTO NO ES `status.success()`. Ver `PS_EPILOGO`: un error no
    // terminante de PowerShell se ve, se escribe, y el proceso sale con cero.
    let (err, ok) = veredicto(&decode_console(&out.stderr), out.status.success());
    Ok((decode_console(&out.stdout), err, ok))
}

/// Traduce una etiqueta de ejecución al script de PowerShell que la cumple.
///
/// EL FALLO QUE ESTO CIERRA. El shell nativo metía TODAS las etiquetas de
/// ejecución en el mismo sitio y luego mandaba su contenido a PowerShell tal
/// cual. Con `<EXECUTE>` eso es correcto. Con las demás no:
///
///   • `<EXECUTE_REG>query HKLM\...` — en PowerShell, `query` es el programa de
///     Terminal Services, no `reg.exe`. No falla: hace otra cosa.
///   • `<EXECUTE_WMIC>cpu get name` — `cpu` no es nada, y el error que sale
///     ("término no reconocido") no se parece a la causa.
///   • `<EXECUTE_NETSH>`, `<EXECUTE_CSCRIPT>` — igual, sin su binario delante.
///
/// El panel decía "Ejecutar (EXECUTE_REG)" y ejecutaba otra cosa. Eso es peor
/// que no soportar la etiqueta, porque el registro de auditoría queda diciendo
/// que se hizo lo que se pidió.
///
/// Sale `None` para las que este shell NO puede cumplir —hoy, la remota—. Quien
/// llama tiene que decirlo, no inventarse un equivalente local: correr en esta
/// máquina un comando que el modelo pidió para otra es el peor final posible.
///
/// El contenido se pasa como ARGUMENTOS de un `&`-call, no interpolado en una
/// cadena de script: así un `;` o un `|` dentro del comando son texto para el
/// binario invocado y no sintaxis para PowerShell.
pub fn tag_to_script(kind: crate::tags::TagKind, content: &str) -> Option<String> {
    use crate::tags::TagKind as K;
    let c = content.trim();
    Some(match kind {
        K::Execute => c.to_string(),
        // `cmd /c` y no `&`: lo que hay dentro suele ser una línea de cmd con su
        // propia sintaxis —tuberías, `&&`, redirecciones— y quien tiene que
        // interpretarla es cmd.
        K::ExecuteCmd => format!("cmd.exe /c {c}"),
        K::ExecuteWmic => format!("wmic.exe {c}"),
        K::ExecuteNetsh => format!("netsh.exe {c}"),
        K::ExecuteReg => format!("reg.exe {c}"),
        K::ExecuteCscript => format!("cscript.exe //NoLogo {c}"),
        // La remota SÍ, y su contenido es el script tal cual: quien lo ejecuta
        // es `hosts::run_remote`, contra el equipo que diga el atributo
        // `target`. Aquí decía «necesita una sesión que este shell todavía no
        // abre», y era verdad hasta que se migró NexShell — el registro de
        // equipos y la ejecución remota existen desde entonces, y el comentario
        // se quedó afirmando una carencia que ya no estaba.
        K::ExecuteRemote => c.to_string(),
        // Ni `<TOOL>` ni `<THOUGHT>` son ejecuciones: van a Trace.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_etiqueta_llama_a_su_binario_y_no_a_powershell_a_secas() {
        use crate::tags::TagKind as K;
        // Sin el prefijo, PowerShell entiende `query` como el programa de
        // Terminal Services: no da error y hace otra cosa. Es el fallo que
        // dejaba el panel diciendo que se ejecutó lo que se pidió.
        assert_eq!(
            tag_to_script(K::ExecuteReg, "query HKLM\\SOFTWARE /v X").unwrap(),
            "reg.exe query HKLM\\SOFTWARE /v X"
        );
        assert_eq!(tag_to_script(K::ExecuteWmic, "cpu get name").unwrap(), "wmic.exe cpu get name");
        assert_eq!(
            tag_to_script(K::ExecuteNetsh, "interface ip show config").unwrap(),
            "netsh.exe interface ip show config"
        );
        assert!(tag_to_script(K::ExecuteCscript, "x.vbs").unwrap().contains("//NoLogo"));
        // La sencilla se queda como está: ya es PowerShell.
        assert_eq!(tag_to_script(K::Execute, "Get-Service").unwrap(), "Get-Service");
    }

    #[test]
    fn lo_que_no_es_una_ejecucion_devuelve_nada() {
        use crate::tags::TagKind as K;
        assert!(tag_to_script(K::Tool, "sysinfo").is_none());
        assert!(tag_to_script(K::Thought, "pensando").is_none());
    }

    #[test]
    fn la_remota_devuelve_su_script_sin_tocarlo() {
        use crate::tags::TagKind as K;
        // Ya se puede cumplir: el registro de equipos y `hosts::run_remote`
        // existen desde que se migró NexShell. Antes esto devolvía `None` y el
        // comentario decía que hacía falta «una sesión que este shell todavía no
        // abre» — cierto cuando se escribió, falso desde entonces.
        //
        // SIN TOCARLO, y eso importa: el script va a correr en la otra máquina,
        // que puede ser un Linux. Envolverlo en algo de PowerShell aquí sería
        // traducirlo al shell equivocado.
        assert_eq!(tag_to_script(K::ExecuteRemote, "systemctl status nginx").unwrap(), "systemctl status nginx");
        assert_eq!(tag_to_script(K::ExecuteRemote, "Get-Service").unwrap(), "Get-Service");
    }

    // Las letras acentuadas de 0x80–0xA5 son IDÉNTICAS en CP-437 (Windows en
    // inglés, que es lo que corre el CI) y CP-850 (español, donde se encontró
    // esto). Por eso estos fixtures afirman lo mismo en las dos.
    const OEM_DISEÑO: &[u8] = &[b'D', b'i', b's', b'e', 0xA4, b'o']; // ñ = 0xA4
    const OEM_ACCENTS: &[u8] = &[0xA0, 0x82, 0xA1, 0xA2, 0xA3]; // á é í ó ú

    #[test]
    fn ascii_survives_untouched() {
        assert_eq!(decode_console(b"Image Name    PID"), "Image Name    PID");
        assert_eq!(decode_console(b""), "");
    }

    #[test]
    fn real_utf8_is_not_mangled_by_the_oem_path() {
        // Algunas herramientas SÍ emiten UTF-8. Probar UTF-8 primero es lo que
        // las mantiene funcionando: decodificarlas como OEM convertiría cada
        // carácter multibyte en dos o tres equivocados.
        let utf8 = "café — ñandú".as_bytes();
        assert_eq!(decode_console(utf8), "café — ñandú");
    }

    #[test]
    #[cfg(windows)]
    fn oem_bytes_decode_to_the_letters_the_tool_printed() {
        assert_eq!(decode_console(OEM_DISEÑO), "Diseño");
        assert_eq!(decode_console(OEM_ACCENTS), "áéíóú");
    }

    #[test]
    fn the_naive_decoder_really_did_corrupt_these_bytes() {
        // Fija la PREMISA, no el arreglo. Si un Windows o un Rust futuro
        // hicieran que `from_utf8_lossy` acertara con esto, el trabajo de arriba
        // sobraría — y aquí es donde se vería.
        let old = String::from_utf8_lossy(OEM_DISEÑO);
        assert!(old.contains('\u{FFFD}'), "esperaba caracteres de reemplazo: {old:?}");
        assert_ne!(old, "Diseño");
    }

    #[test]
    fn the_preamble_sets_both_handles_before_the_payload() {
        let w = ps_utf8("Get-Date");
        assert!(w.starts_with("[Console]::OutputEncoding"), "el preámbulo va primero");
        assert!(w.contains("$OutputEncoding"), "falta el segundo manejador");
        let script_at = w.find("Get-Date").expect("el script debe seguir ahí");
        let out_enc_at = w.find("$OutputEncoding").expect("...");
        assert!(out_enc_at < script_at, "la codificación se fija en la primera escritura");
    }
}

#[cfg(test)]
mod veredicto_de_powershell {
    use super::*;

    /// Lo que deja PowerShell en stderr tras un error mostrado, con su marcador.
    fn con_error(errores: u32, texto: &str) -> String {
        format!("{texto}\n{FIN}{errores}\n")
    }

    #[test]
    fn un_error_no_terminante_ya_no_pasa_por_exito() {
        // EL CASO MEDIDO: `Get-Item 'C:\no-existe'; Get-Date` sale con CERO y el
        // error esta ahi. Lucy lo apuntaba como exito, el paso iba a «hecho», la
        // auditoria guardaba exit_code 0 y `fallos_recientes` no lo veia nunca.
        let (limpio, ok) = veredicto(
            &con_error(1, "Get-Item : No se encuentra la ruta de acceso."),
            true,
        );
        assert!(!ok, "un error visible sigue contando como exito");
        assert!(!limpio.contains(FIN), "el marcador llega al operador");
        assert!(limpio.contains("No se encuentra"), "se perdio el mensaje del error");
    }

    #[test]
    fn un_programa_nativo_que_avisa_por_stderr_sigue_yendo_bien() {
        // `git`, `winget` y media consola de Windows escriben en stderr sin que
        // haya fallado nada. Sin `$Error`, marcarlos como fallo seria una maquina
        // de falsos positivos — y con `fallos_recientes` conectado, ademas le
        // diria al modelo que reintente lo que si funciono.
        let (_, ok) = veredicto(&con_error(0, "Cloning into 'repo'..."), true);
        assert!(ok, "el aviso de un programa nativo cuenta como fallo");
    }

    #[test]
    fn un_error_silenciado_a_proposito_no_es_un_fallo() {
        // `-ErrorAction SilentlyContinue` sube `$Error` y NO escribe en stderr:
        // quien lo puso dijo que ese error no importa.
        let (_, ok) = veredicto(&format!("{FIN}1\n"), true);
        assert!(ok, "un error que alguien silencio cuenta como fallo");
    }

    #[test]
    fn lo_que_ya_fallaba_sigue_fallando() {
        // Un programa nativo que termina mal SI llega al codigo de salida del
        // proceso, y ese camino no cambia.
        let (_, ok) = veredicto(&format!("{FIN}0\n"), false);
        assert!(!ok, "se perdio el fallo que el proceso si reportaba");
    }

    #[test]
    fn sin_marcador_se_cree_lo_que_dijo_el_proceso() {
        // El script llamo a `exit` y el epilogo no llego a correr. Ahi el codigo
        // que puso el script ES el veredicto correcto.
        assert_eq!(veredicto("lo que sea", true), ("lo que sea".into(), true));
        assert_eq!(veredicto("", false), (String::new(), false));
    }

    #[test]
    fn el_epilogo_va_detras_del_script_y_en_su_propia_linea() {
        // Un script que acaba en comentario se comeria la linea siguiente si el
        // epilogo se pegara sin salto.
        let s = ps_utf8("Get-Date  # mira la hora");
        assert!(s.contains("# mira la hora\n"), "el epilogo se pego al comentario");
        // Y lo ULTIMO que hace es reponer el codigo de salida. Sin esa linea el
        // epilogo se convierte en la ultima instruccion del script y PowerShell
        // saca de ella su codigo: un `cmd /c exit 3` pasaria a salir con cero.
        assert!(
            s.trim_end().ends_with("if ($__lucy_code) { exit $__lucy_code }"),
            "el epilogo ya no repone el codigo de salida: {s}"
        );
    }
}

#[cfg(all(test, windows))]
mod contra_powershell_de_verdad {
    use super::*;

    /// Los cinco casos de la tabla, lanzando PowerShell.
    ///
    /// LAS PRUEBAS DE ARRIBA FIJAN EL PARSER; ESTA FIJA EL ARREGLO. Un parser
    /// correcto sobre un marcador que PowerShell no escribiera —porque el
    /// epilogo se pego mal, o porque `$Error` no dice lo que se cree— pasaria
    /// igual de verde y no arreglaria nada.
    #[test]
    fn los_cinco_casos_medidos() {
        let casos: &[(&str, bool, &str)] = &[
            ("Get-Date", true, "un comando que va bien"),
            (
                "Get-Item 'C:\no-existe-lucy-xyz'; Get-Date",
                false,
                "error NO terminante: sale con cero y hay que cazarlo",
            ),
            (
                "Get-Item 'C:\no-existe-lucy-xyz' -ErrorAction SilentlyContinue; Get-Date",
                true,
                "error silenciado a proposito",
            ),
            (
                "cmd /c 'echo aviso 1>&2'",
                true,
                "programa nativo que escribe en stderr y va bien",
            ),
            ("cmd /c exit 3", false, "programa nativo que falla"),
        ];
        for (script, esperado, que) in casos {
            let (_, err, ok) = run_powershell_utf8(script).expect("arrancar PowerShell");
            assert_eq!(ok, *esperado, "{que} — script: {script}, stderr: {err}");
            assert!(!err.contains("LUCY-FIN"), "{que}: el marcador llega fuera");
        }
    }
}
