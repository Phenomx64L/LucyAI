//! Ejecutar un comando COMO ADMINISTRADOR, con su UAC.
//!
//! POR QUÉ LOS FLUJOS DE LA V1 Y LA V2 NO FUNCIONABAN. Elevar en Windows no es
//! un parámetro: es lanzar un proceso NUEVO con `Start-Process -Verb RunAs`, y
//! ese proceso corre en otra sesión de integridad con sus propios descriptores
//! de salida. El padre —sin elevar— NO PUEDE leer su stdout. Cualquier intento
//! de capturarlo directamente devuelve vacío, que es exactamente el síntoma de
//! "se ejecuta pero no trae nada".
//!
//! La solución es la única que hay: el hijo escribe su salida a un FICHERO, y el
//! padre lo lee cuando el hijo termina. Suena rodeo y no lo es — es el
//! mecanismo, y no conocerlo es lo que hace que este flujo se implemente tres
//! veces y las tres se quede a medias.
//!
//! Y EL COMANDO VIAJA EN UN `.ps1`, no en la línea de órdenes. Meterlo como
//! argumento obliga a escapar comillas dentro de comillas dentro de
//! `-ArgumentList`, y un comando de PowerShell real —con `Where-Object { $_.X
//! -eq 'Y' }`— lleva las tres clases a la vez. Con un fichero no hay nada que
//! escapar: el comando se escribe tal cual y se ejecuta tal cual.

use std::path::PathBuf;

/// Dónde se dejan el script y su salida.
///
/// En el temporal del usuario y con el id del proceso en el nombre: dos
/// ventanas de Lucy abiertas a la vez no pueden pisarse el fichero.
fn temp_pair() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir();
    let stamp = format!("{}-{:?}", std::process::id(), std::time::Instant::now());
    let key: u64 = stamp.bytes().fold(1469598103934665603, |h, b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    });
    (
        dir.join(format!("lucy-elev-{key:x}.ps1")),
        dir.join(format!("lucy-elev-{key:x}.out")),
    )
}

/// El contenido del `.ps1` que correrá elevado.
///
/// `*>` redirige TODOS los flujos —salida, errores, avisos, verboso— al mismo
/// fichero. Con solo `>` el error de un comando fallido se quedaría en una
/// consola que se cierra al instante, y el operador vería un fichero vacío
/// creyendo que no pasó nada.
pub fn build_script(cmd: &str, out: &std::path::Path) -> String {
    format!(
        "$ErrorActionPreference = 'Continue'\r\n\
         & {{\r\n{cmd}\r\n}} *> '{}'\r\n\
         exit $LASTEXITCODE\r\n",
        out.display()
    )
}

/// El script en base64 UTF-16LE, que es lo que come `-EncodedCommand`.
///
/// Es el MISMO envoltorio que usa `hosts::run_remote` para WinRM, y por la misma
/// razón: dentro de base64 no hay comillas, ni llaves, ni nada que pueda cerrar
/// lo que lo rodea.
fn encoded(script: &str) -> String {
    let mut bytes = Vec::with_capacity(script.len() * 2);
    for u in script.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    crate::attach::b64_encode(&bytes)
}

/// El comando que el padre lanza para pedir la elevación.
///
/// `-Wait` es obligatorio: sin él, `Start-Process` vuelve en cuanto UAC acepta y
/// el padre leería el fichero de salida antes de que el hijo escriba nada.
/// `-PassThru` da el proceso para poder mirar su código de salida.
///
/// EL SCRIPT VA DENTRO Y NO EN UN FICHERO, y esto cierra una escalada de
/// privilegios. Antes se escribía un `.ps1` en el temporal del usuario y se
/// lanzaba con `-File`. Entre la escritura y el momento en que el hijo ELEVADO
/// lo lee hay un hueco, y en ese hueco cabe el diálogo de UAC entero — los
/// segundos que el operador tarda en leerlo y pulsar «Sí».
///
/// El temporal del usuario lo puede escribir cualquier proceso que corra COMO
/// ESE USUARIO, y esos procesos NO son administradores: ahí está el ataque.
/// Otro programa cambia el contenido del `.ps1` mientras el operador lee el
/// aviso, el operador consiente para el comando que él pidió, y lo que se
/// ejecuta con permisos de administrador es otra cosa. El diálogo de UAC dice
/// «powershell.exe» en los dos casos, así que no hay nada que mirar.
///
/// Es la razón entera de que UAC exista, y se saltaba con un `fs::write`.
///
/// SE CONSERVA LO QUE MOTIVÓ EL FICHERO. La cabecera del módulo explica que el
/// `.ps1` estaba para no escapar comillas dentro de comillas dentro de
/// `-ArgumentList`, y un comando real las lleva de las tres clases. Base64 lo
/// resuelve mejor: su alfabeto es `A-Za-z0-9+/=`, así que no hay nada que
/// escapar y no hay fichero que cambiar.
///
/// El fichero de SALIDA se queda, porque ahí no hay elección: un proceso
/// elevado corre en otra sesión y el padre no puede leer su stdout. Y no vale
/// lo mismo: lo escribe el hijo, el padre lo lee cuando el hijo ya terminó, y
/// cambiarlo solo le enseña al operador una salida falsa — no ejecuta nada.
pub fn build_launcher(script: &str) -> String {
    format!(
        "$p = Start-Process powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
         -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-EncodedCommand','{}'; \
         exit $p.ExitCode",
        encoded(script)
    )
}

/// Corre `cmd` elevado y devuelve `(salida, ok)`.
///
/// Un UAC cancelado NO es un fallo del comando y se distingue: el operador dijo
/// que no, y decirle "el comando falló" sería culpar a la herramienta de una
/// decisión suya.
#[cfg(windows)]
pub fn run_elevated(cmd: &str) -> Result<(String, bool), String> {
    let (_ps, out) = temp_pair();
    // YA NO SE ESCRIBE NINGÚN `.ps1`. El script viaja dentro del propio
    // lanzador, en base64, para que no haya fichero que otro proceso pueda
    // cambiar mientras el operador lee el aviso de UAC. Ver `build_launcher`.
    let r = crate::shell::run_powershell_utf8(&build_launcher(&build_script(cmd, &out)));
    let salida = std::fs::read_to_string(&out).unwrap_or_default();
    // El de salida SÍ se limpia: lleva dentro lo que devolvió un comando del
    // operador y no tiene por qué quedarse en el temporal.
    let _ = std::fs::remove_file(&out);

    match r {
        Ok((_, err, ok)) => {
            if !ok && salida.trim().is_empty() {
                // `Start-Process -Verb RunAs` falla con esto cuando el usuario
                // cierra el diálogo de UAC.
                let cancelado = err.contains("cancel") || err.contains("canceló")
                    || err.contains("1223");
                return Ok((
                    if cancelado {
                        "El operador canceló la elevación: el comando no se ejecutó.".into()
                    } else {
                        format!("No se pudo elevar: {}", err.trim())
                    },
                    false,
                ));
            }
            Ok((salida, ok))
        }
        Err(e) => Err(e),
    }
}

#[cfg(not(windows))]
pub fn run_elevated(_cmd: &str) -> Result<(String, bool), String> {
    Err("La elevación solo existe en Windows".into())
}

/// Qué se puede hacer con la elevación en ESTE equipo y ESTA sesión.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    /// El proceso YA tiene privilegios de administrador. Pedir elevación no
    /// haría nada: si un comando falló por permisos, no es por eso.
    Already,
    /// Sin privilegios, pero con UAC activo: `RunAs` lanzará el diálogo.
    CanPrompt,
    /// Sin privilegios y sin UAC. No hay forma de elevar sobre la marcha: la
    /// cuenta no es administradora y el mecanismo de consentimiento está
    /// apagado. Hay que abrir Lucy con otra cuenta.
    Unavailable,
}

/// La consulta que responde las dos preguntas de una vez.
///
/// `IsInRole(Administrator)` es exactamente lo que se necesita y no la
/// comprobación de grupo a secas: devuelve verdadero cuando el token está
/// ELEVADO, o cuando UAC está apagado y la cuenta es administradora — es decir,
/// "¿mando en esta máquina AHORA MISMO?". `EnableLUA` distingue los dos noes.
pub const PROBE: &str = "$e = ([Security.Principal.WindowsPrincipal]\
    [Security.Principal.WindowsIdentity]::GetCurrent())\
    .IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator); \
    $u = (Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System' \
    -Name EnableLUA -ErrorAction SilentlyContinue).EnableLUA; \
    Write-Output \"$e|$u\"";

/// Interpreta la salida de `PROBE`.
///
/// Ante una lectura que no se entiende se asume `CanPrompt`: ofrecer un botón
/// que quizá no haga falta cuesta un clic, y esconderlo cuando sí hacía falta
/// deja al operador sin salida.
pub fn parse_probe(out: &str) -> Elevation {
    let s = out.trim().to_lowercase();
    let (admin, uac) = s.split_once('|').unwrap_or((s.as_str(), ""));
    if admin.trim() == "true" {
        return Elevation::Already;
    }
    // `EnableLUA` ausente significa activo: es el valor por defecto de Windows,
    // y solo aparece explícitamente cuando alguien lo tocó.
    if uac.trim() == "0" {
        Elevation::Unavailable
    } else {
        Elevation::CanPrompt
    }
}

/// Estado de elevación de esta sesión, consultado UNA vez.
///
/// Una vez porque no cambia mientras el proceso vive: ni el token ni la
/// política de UAC se mueven bajo los pies de una aplicación en marcha, y la
/// consulta lanza un PowerShell que no hay por qué repetir en cada frame.
#[cfg(windows)]
pub fn state() -> Elevation {
    use std::sync::OnceLock;
    static S: OnceLock<Elevation> = OnceLock::new();
    *S.get_or_init(|| match crate::shell::run_powershell_utf8(PROBE) {
        Ok((out, _, _)) => parse_probe(&out),
        Err(_) => Elevation::CanPrompt,
    })
}

#[cfg(not(windows))]
pub fn state() -> Elevation {
    Elevation::Unavailable
}

/// ¿La salida de un comando dice que le faltaron privilegios?
///
/// Sirve para OFRECER la elevación después de un fallo, en vez de pedirla por
/// adelantado. Un UAC que salta antes de saber si hace falta enseña a aceptarlo
/// sin leerlo, y ese hábito es peor que el comando que se quería correr.
pub fn looks_like_access_denied(output: &str) -> bool {
    let o = output.to_lowercase();
    [
        "access is denied",
        "acceso denegado",
        "no se puede abrir el servicio",
        "cannot open",
        "openerror",
        "requires elevation",
        "requiere elevación",
        "unauthorizedaccess",
        "permissiondenied",
        "5)", // el código 5 de Win32: ERROR_ACCESS_DENIED
    ]
    .iter()
    .any(|m| o.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn el_script_redirige_todos_los_flujos() {
        // Con solo `>` el error de un comando fallido se queda en una consola
        // que se cierra al instante, y el operador ve un fichero vacío creyendo
        // que no pasó nada.
        let s = build_script("Get-Service", Path::new("C:/tmp/o.txt"));
        assert!(s.contains("*>"), "no redirige todos los flujos: {s}");
        assert!(s.contains("Get-Service"));
        assert!(s.contains("C:/tmp/o.txt"));
    }

    #[test]
    fn el_comando_va_dentro_del_script_sin_escapar_nada() {
        // Un comando real lleva comillas simples, dobles y llaves a la vez.
        // Como argumento habría que escaparlas tres veces; en un fichero se
        // escribe tal cual, y ESA es la razón de usar un `.ps1`.
        let cmd = "Get-Service | Where-Object { $_.Status -eq 'Stopped' -and $_.Name -ne \"x\" }";
        let s = build_script(cmd, Path::new("C:/tmp/o.txt"));
        assert!(s.contains(cmd), "el comando se alteró al construir el script");
    }

    #[test]
    fn el_lanzador_espera_al_hijo() {
        // Sin `-Wait`, `Start-Process` vuelve en cuanto UAC acepta y el padre
        // leería el fichero antes de que el hijo escriba nada — el fallo que
        // deja "se ejecutó pero no trajo nada".
        let l = build_launcher("Get-Date");
        assert!(l.contains("-Wait"), "sin -Wait se lee el fichero vacío");
        assert!(l.contains("-Verb RunAs"), "sin RunAs no hay elevación");
    }

    #[test]
    fn el_script_no_viaja_por_un_fichero_que_otro_pueda_cambiar() {
        // LA ESCALADA QUE ESTO CIERRA. El script se escribía en el temporal del
        // usuario y se lanzaba con `-File`. Entre escribirlo y que el hijo
        // ELEVADO lo leyera cabía el diálogo de UAC entero — los segundos que el
        // operador tarda en leerlo. Y ese temporal lo puede escribir cualquier
        // proceso que corra como ese usuario, que NO es administrador.
        //
        // Resultado: otro programa cambia el fichero mientras el operador lee el
        // aviso, el operador consiente para SU comando, y con permisos de
        // administrador corre otra cosa. El diálogo dice «powershell.exe» en los
        // dos casos.
        let l = build_launcher("Get-Date");
        assert!(!l.contains("-File"), "el script vuelve a viajar por fichero");
        assert!(l.contains("-EncodedCommand"), "el script no va dentro del lanzador");
        assert!(!l.contains(".ps1"), "sigue nombrando un fichero de script");
    }

    #[test]
    fn el_comando_sobrevive_al_ida_y_vuelta_de_base64() {
        // Base64 sustituye al fichero, y el fichero estaba para no escapar
        // comillas dentro de comillas. Si el envoltorio nuevo alterara el
        // comando, habríamos cambiado un agujero por un fallo.
        let cmd = "Get-Service | Where-Object { $_.Status -eq 'Stopped' -and $_.Name -ne \"x\" }";
        let dentro = build_script(cmd, Path::new("C:/tmp/o.txt"));
        let b64 = encoded(&dentro);
        // El alfabeto de base64 no lleva comillas ni llaves: nada de lo que va
        // dentro puede cerrar el `'…'` que lo envuelve en `-ArgumentList`.
        assert!(
            b64.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='),
            "el envoltorio dejó pasar un carácter que puede cerrar la cadena"
        );
        // Y se descodifica a lo mismo: UTF-16LE, dos bytes por unidad.
        let bytes = b64_decode_prueba(&b64);
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(String::from_utf16(&u16s).unwrap(), dentro);
    }

    /// Descodificador de base64 SOLO PARA EL TEST. En producción nunca se
    /// descodifica —los bytes salen hacia PowerShell y ya— así que tenerlo en el
    /// binario sería código que nadie ejecuta.
    fn b64_decode_prueba(s: &str) -> Vec<u8> {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let val = |c: u8| A.iter().position(|x| *x == c).map(|i| i as u32);
        let mut out = Vec::new();
        let limpio: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
        for trozo in limpio.chunks(4) {
            let mut n = 0u32;
            for (i, b) in trozo.iter().enumerate() {
                n |= val(*b).expect("carácter fuera del alfabeto") << (18 - 6 * i);
            }
            let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            out.extend_from_slice(&bytes[..trozo.len() - 1]);
        }
        out
    }

    #[test]
    fn se_reconoce_un_acceso_denegado_en_los_dos_idiomas() {
        // Windows responde en el idioma del sistema, y esta app corre en
        // español: mirar solo el inglés es cómo se pierde la mitad de los casos.
        assert!(looks_like_access_denied("Access is denied"));
        assert!(looks_like_access_denied("Acceso denegado."));
        assert!(looks_like_access_denied(
            "Start-Service : No se puede abrir el servicio gpsvc en el equipo"
        ));
        assert!(looks_like_access_denied("Error: OpenError"));
        assert!(!looks_like_access_denied("El servicio está detenido"));
        assert!(!looks_like_access_denied(""));
    }

    #[test]
    fn dos_ejecuciones_no_comparten_fichero() {
        // Dos ventanas de Lucy abiertas a la vez no pueden pisarse la salida.
        let (a, _) = temp_pair();
        let (b, _) = temp_pair();
        assert_ne!(a, b);
    }
}

#[cfg(test)]
mod estado {
    use super::*;

    #[test]
    fn ya_administrador_no_necesita_pedir_nada() {
        // Con UAC apagado y cuenta de administrador, `IsInRole` da true: el
        // proceso YA manda. Ofrecer "reintentar como administrador" ahí sería
        // una promesa falsa — el reintento fallaría igual, porque lo que falló
        // no fueron los permisos.
        assert_eq!(parse_probe("True|0"), Elevation::Already);
        assert_eq!(parse_probe("True|1"), Elevation::Already);
        assert_eq!(parse_probe("true|"), Elevation::Already);
    }

    #[test]
    fn sin_privilegios_y_con_uac_se_puede_pedir() {
        assert_eq!(parse_probe("False|1"), Elevation::CanPrompt);
        // `EnableLUA` ausente es UAC ACTIVO: es el valor por defecto de Windows
        // y solo aparece escrito cuando alguien lo cambió.
        assert_eq!(parse_probe("False|"), Elevation::CanPrompt);
    }

    #[test]
    fn sin_privilegios_y_sin_uac_no_hay_nada_que_pedir() {
        // Cuenta estándar con el consentimiento apagado: no existe forma de
        // elevar sobre la marcha, y el botón mentiría. Hay que abrir Lucy con
        // otra cuenta, y eso es lo que hay que decir.
        assert_eq!(parse_probe("False|0"), Elevation::Unavailable);
    }

    #[test]
    fn una_lectura_ilegible_prefiere_ofrecer_el_boton() {
        // Ofrecer un botón que quizá no hacía falta cuesta un clic; esconderlo
        // cuando sí hacía falta deja al operador sin salida.
        assert_eq!(parse_probe(""), Elevation::CanPrompt);
        assert_eq!(parse_probe("basura"), Elevation::CanPrompt);
    }
}
