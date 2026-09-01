//! Checks de CIS Benchmark: qué está mal configurado en un equipo.
//!
//! Veinte comprobaciones por sistema —contraseñas, cortafuegos, SSH, registro de
//! eventos, cifrado— cada una con el comando que la mide, lo que se espera de él
//! y cómo se arregla si falla.
//!
//! EL CATÁLOGO SE COMPARTE CON LA APP, no se copia. Los dos JSON viven en
//! `src/lib/compliance/` y este módulo los mete en el binario con `include_str!`.
//! Copiarlos aquí habría dado dos listas que empiezan iguales y acaban
//! discrepando en el check que alguien corrigió en un solo lado — y en un panel
//! de cumplimiento eso significa que las dos mitades del programa dan dos notas
//! distintas del mismo servidor.
//!
//! LA EVIDENCIA NO SE MUTILA, y ésta es la diferencia de fondo con la V2. Para
//! proteger un JSON fabricado a mano, su camino de Linux hace
//! `tr '"\\' '..'`: sustituye por PUNTOS todas las comillas y las barras
//! invertidas de la salida del check. O sea que la prueba de que
//! `PermitRootLogin no` está bien puesto llega como `PermitRootLogin no` si hay
//! suerte y como `.etc.ssh.sshd_config` si la ruta salía en la línea. El
//! comentario del camino local de esa misma versión dice, sobre otra cosa, que
//! «una evidencia corrompida es peor que una ausente: parece evidencia de
//! verdad». Tiene razón, y por eso aquí el escapado es REVERSIBLE.
//!
//! UN CHECK QUE NO SE PUDO EJECUTAR NO ES UN CHECK QUE FALLA. Si falta la
//! herramienta, o no hay permisos, decir «FALLA» afirma que el equipo está mal
//! configurado cuando lo que pasa es que no se pudo mirar. Son tres estados y no
//! dos, por el mismo motivo por el que el inventario enseña un guion y no un
//! cero.

use serde::Deserialize;

/// El separador de campos, el mismo que el inventario y por el mismo motivo.
pub const US: char = '\u{1f}';
pub const MARCA: &str = "LUCY:chk";

/// Cuánto se guarda de la salida de un check.
///
/// La evidencia es para poder decidir si el check acertó, no un volcado: un
/// `Get-GPResultantSetOfPolicy` son cien kilobytes y lo que importa está en la
/// primera línea. La V2 corta en 1000 y se queda.
pub const MAX_EVIDENCIA: usize = 1_200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severidad {
    // El orden importa: `derive(Ord)` lo usa para ordenar la tabla por gravedad,
    // y lo que se quiere ver arriba es lo crítico.
    Critical,
    High,
    Medium,
    Low,
}

impl Severidad {
    pub fn label(self) -> &'static str {
        match self {
            Self::Critical => "crítica",
            Self::High => "alta",
            Self::Medium => "media",
            Self::Low => "baja",
        }
    }
}

/// Qué hace que un check se dé por bueno.
///
/// DOS FORMAS Y NO UNA. Diecisiete de los veinte comprueban que la salida
/// contenga un texto; tres miran el código de salida. Implementar solo la
/// primera dejaba esos tres fallando siempre —comparando contra una aguja
/// vacía— sobre equipos perfectamente configurados.
#[derive(Debug, Clone, PartialEq)]
pub enum Espera {
    Contiene(String),
    Salida(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub id: String,
    pub title: String,
    pub category: String,
    pub severity: Severidad,
    pub command: String,
    pub espera: Espera,
    pub remediation: String,
}

/// La forma exacta del JSON compartido. Se lee y se convierte a `Check`.
#[derive(Deserialize)]
struct CheckJson {
    id: String,
    title: String,
    category: String,
    severity: Severidad,
    command: String,
    expect: EsperaJson,
    #[serde(default)]
    remediation: String,
}

#[derive(Deserialize)]
struct EsperaJson {
    #[serde(default)]
    stdout_contains: Option<String>,
    #[serde(default)]
    exit_code: Option<i32>,
}

/// LOS CATÁLOGOS SON DEL NÚCLEO, y ahora viven aquí.
///
/// Estaban en `src/lib/compliance/` — la carpeta del frontend SvelteKit— y se
/// traían con un `include_str!` que subía dos niveles y se metía en el otro
/// proyecto. Eso hacía que `lucy-core` NO COMPILARA sin el repositorio de la V1
/// al lado: no una dependencia de datos que se pudiera echar de menos en tiempo
/// de ejecución, sino un error del compilador. El «corazón sin Tauri» no
/// arrancaba sin la mitad Tauri.
///
/// Y el sitio era el equivocado por más de una razón. Una regla CIS no es
/// presentación: dice qué comando se ejecuta en la máquina de alguien y qué
/// salida se considera conforme. Que viviera en la carpeta de los componentes de
/// interfaz significaba que quien reorganizara el frontend podía moverla sin
/// enterarse de que estaba rompiendo el motor de cumplimiento.
///
/// El frontend de la V1 las sigue leyendo, ahora desde aquí.
const CIS_WINDOWS: &str = include_str!("../assets/compliance/cis-windows.json");
const CIS_LINUX: &str = include_str!("../assets/compliance/cis-linux.json");

/// El catálogo que le toca a un equipo.
pub fn catalogo(windows: bool) -> Vec<Check> {
    let crudo = if windows { CIS_WINDOWS } else { CIS_LINUX };
    let js: Vec<CheckJson> = serde_json::from_str(crudo).unwrap_or_default();
    js.into_iter()
        .filter_map(|c| {
            // Un check sin forma de evaluarse se DESCARTA en vez de darse por
            // bueno: una fila verde que nadie ha comprobado es peor que una fila
            // que falta, porque cuenta para el porcentaje.
            let espera = match (c.expect.stdout_contains, c.expect.exit_code) {
                (Some(s), _) if !s.is_empty() => Espera::Contiene(s),
                (_, Some(e)) => Espera::Salida(e),
                _ => return None,
            };
            Some(Check {
                id: c.id,
                title: c.title,
                category: c.category,
                severity: c.severity,
                command: c.command,
                espera,
                remediation: c.remediation,
            })
        })
        .collect()
}

/// El catálogo que le toca a este equipo remoto, o por qué no se puede.
pub fn catalogo_de(h: &crate::hosts::Host) -> Result<Vec<Check>, String> {
    if !h.protocol.can_shell() {
        return Err(format!(
            "«{}» está dado de alta como {} y por ahí no se pueden pasar checks.",
            h.name,
            h.protocol.label()
        ));
    }
    Ok(catalogo(h.protocol == crate::hosts::Protocol::Winrm))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Estado {
    Pasa,
    /// Falló, pero es de gravedad media o baja.
    ///
    /// LA GRAVEDAD DECIDE SI UN FALLO ES UN FALLO O UN AVISO, y sin esa
    /// separación la lista es inútil en cuanto pasa de diez filas: «faltan
    /// actualizaciones» y «SMBv1 sigue habilitado» salen del mismo color y con la
    /// misma urgencia, así que se leen las dos con la misma prisa — que en la
    /// práctica es ninguna.
    Aviso,
    Falla,
    /// No se pudo ejecutar: falta la herramienta, faltan permisos, el comando no
    /// existe en esta distribución, o la salida vino vacía. NO es lo mismo que
    /// fallar: fallar es un hecho sobre el equipo, esto es un hecho sobre la
    /// medición.
    Error,
}

impl Estado {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pasa => "OK",
            Self::Aviso => "Aviso",
            Self::Falla => "Falla",
            Self::Error => "No se pudo",
        }
    }

    /// Si cuenta como cumplido para la nota.
    pub fn conforme(self) -> bool {
        self == Self::Pasa
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resultado {
    pub check: Check,
    pub estado: Estado,
    /// Lo que devolvió el comando, entero y sin sustituir nada.
    pub evidencia: String,
    pub exit_code: i32,
}

/// El porcentaje de aprobados, sobre los que SE PUDIERON MEDIR.
///
/// Los que no se pudieron ejecutar quedan fuera del denominador. Contarlos como
/// fallos hundiría la nota por un problema de permisos, y contarlos como
/// aprobados la inflaría por lo mismo: en los dos casos el número diría algo
/// sobre el equipo que nadie ha comprobado. Se devuelve también cuántos son,
/// para que la vista pueda enseñarlo al lado.
pub fn porcentaje(rs: &[Resultado]) -> (u32, usize) {
    let medidos: Vec<&Resultado> = rs.iter().filter(|r| r.estado != Estado::Error).collect();
    let sin_medir = rs.len() - medidos.len();
    if medidos.is_empty() {
        return (0, sin_medir);
    }
    let pasa = medidos.iter().filter(|r| r.estado == Estado::Pasa).count();
    (((pasa as f64 / medidos.len() as f64) * 100.0).round() as u32, sin_medir)
}

/// El script que corre los checks en un equipo.
///
/// La salida es una línea por check: `LUCY:chk<US>id<US>código<US>evidencia`, con
/// la evidencia escapada de forma REVERSIBLE — `\` pasa a `\\` y el salto de
/// línea a `\n`. Es la diferencia con la V2, que sustituye comillas y barras por
/// puntos para no romper su JSON casero y entrega una evidencia que ya no dice
/// lo que decía.
pub fn script(checks: &[Check], windows: bool) -> String {
    if windows {
        let mut s = String::from("$ErrorActionPreference='Continue'\n");
        for c in checks {
            s.push_str(&format!(
                "try {{ $o = Invoke-Expression '{}' 2>&1 | Out-String; \
                   $e = $LASTEXITCODE; if ($null -eq $e) {{ $e = 0 }} }} \
                 catch {{ $o = $_.Exception.Message; $e = 1 }}\n\
                 $o = ($o -replace '\\\\','\\\\\\\\') -replace \"`r?`n\",'\\n'\n\
                 Write-Output ('{MARCA}'+[char]31+'{}'+[char]31+[string]$e+[char]31+$o)\n",
                crate::hosts::ps_quote(&c.command),
                crate::hosts::ps_quote(&c.id),
            ));
        }
        s
    } else {
        let mut s = String::from("S=$(printf '\\037')\n");
        for c in checks {
            // `sh -c` y no `bash -c`: el transporte ya entrega el script a `sh`,
            // y dar por hecho que hay bash en un Alpine es cómo un check sale
            // «no se pudo» en toda una flota de contenedores.
            s.push_str(&format!(
                "o=$(sh -c '{}' 2>&1); e=$?\n\
                 esc=$(printf '%s' \"$o\" | sed 's/\\\\/\\\\\\\\/g' | awk '{{printf \"%s\\\\n\", $0}}')\n\
                 printf '{MARCA}%s%s%s%s%s%s\\n' \"$S\" '{}' \"$S\" \"$e\" \"$S\" \"$esc\"\n",
                crate::hosts::sh_quote(&c.command),
                crate::hosts::sh_quote(&c.id),
            ));
        }
        s
    }
}

/// Deshace el escapado. `\\` vuelve a ser `\`, `\n` vuelve a ser un salto.
///
/// De una pasada y mirando el carácter siguiente, no con dos `replace`
/// encadenados: `replace("\\\\","\\")` seguido de `replace("\\n","\n")` convierte
/// la secuencia `\\n` —una barra invertida literal seguida de una ene, que es lo
/// que hay en `C:\nueva\ruta`— en un salto de línea.
fn desescapa(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            o.push(c);
            continue;
        }
        match it.next() {
            Some('n') => o.push('\n'),
            Some('\\') => o.push('\\'),
            Some(otro) => {
                o.push('\\');
                o.push(otro);
            }
            None => o.push('\\'),
        }
    }
    o
}

/// Interpreta la salida y evalúa cada check.
///
/// Los checks que NO aparecen en la salida vuelven como `Error`. Es el caso de
/// una sesión que se cortó a media lista, y omitirlos sin más haría que el
/// porcentaje se calculara sobre los que dio tiempo a correr — una nota alta
/// sobre media auditoría.
pub fn parse(salida: &str, checks: &[Check]) -> Vec<Resultado> {
    use std::collections::HashMap;
    let mut vistos: HashMap<&str, (i32, String)> = HashMap::new();
    for l in salida.lines() {
        let Some(resto) = l.trim_end_matches('\r').strip_prefix(MARCA) else { continue };
        let mut campos = resto.split(US);
        // El primer campo va vacío: la línea empieza por el separador.
        let _ = campos.next();
        let (Some(id), Some(ec)) = (campos.next(), campos.next()) else { continue };
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        let ev = desescapa(campos.next().unwrap_or_default());
        if let Some(c) = checks.iter().find(|c| c.id == id) {
            vistos.insert(
                c.id.as_str(),
                (ec.trim().parse().unwrap_or(-1), ev.chars().take(MAX_EVIDENCIA).collect()),
            );
        }
    }
    checks
        .iter()
        .map(|c| match vistos.get(c.id.as_str()) {
            None => Resultado {
                check: c.clone(),
                estado: Estado::Error,
                evidencia: "El equipo no devolvió nada para este check.".into(),
                exit_code: -1,
            },
            Some((ec, ev)) => {
                let estado = evalua(c, *ec, ev);
                Resultado {
                    check: c.clone(),
                    estado,
                    evidencia: ev.clone(),
                    exit_code: *ec,
                }
            }
        })
        .collect()
}

/// Un fallo, con la gravedad ya aplicada.
///
/// Media y baja bajan a aviso. Es la regla de la vista que se migra y es la
/// correcta: un «faltan actualizaciones» y un «SMBv1 sigue habilitado» en la
/// misma lista roja se leen con la misma prisa, que en la práctica es ninguna.
fn fallo(c: &Check) -> Estado {
    match c.severity {
        Severidad::Critical | Severidad::High => Estado::Falla,
        Severidad::Medium | Severidad::Low => Estado::Aviso,
    }
}

fn evalua(c: &Check, ec: i32, ev: &str) -> Estado {
    match &c.espera {
        Espera::Salida(esperado) => {
            if ec == *esperado {
                Estado::Pasa
            } else {
                fallo(c)
            }
        }
        Espera::Contiene(aguja) => {
            if ev.to_lowercase().contains(&aguja.to_lowercase()) {
                return Estado::Pasa;
            }
            // UNA SALIDA VACÍA NO ES UN FALLO: ES UN CHECK QUE NO DECIDIÓ.
            //
            // Encontrado ejecutando esto contra esta máquina. El check W1.1 hace
            // `net accounts | Select-String 'Length of password history' | …`, y
            // en un Windows en español `net accounts` dice «Duración del
            // historial de contraseñas». La tubería no casa nada, no imprime
            // nada, y el código de salida es CERO porque nada ha reventado.
            //
            // Con la regla anterior eso salía FALLA — o sea, el panel afirmaba
            // que este equipo tiene mal la política de contraseñas basándose en
            // una medición que no existió. Tres de los veinte checks caían así, y
            // la nota los contaba.
            if ev.trim().is_empty() {
                return Estado::Error;
            }
            // Y un check que imprime PASS o FAIL pero devuelve un código distinto
            // de cero tampoco llegó a decidir: el comando reventó. Sin esto, una
            // máquina sin `auditpol` sale con doce fallos rojos y parece la peor
            // configurada de la flota.
            if ec != 0 && !ev.to_uppercase().contains("FAIL") {
                Estado::Error
            } else {
                fallo(c)
            }
        }
    }
}

/// Pasa los checks en este equipo.
pub fn run_local(checks: &[Check]) -> Result<Vec<Resultado>, String> {
    let (out, err, _) = crate::shell::run_powershell_utf8(&script(checks, true))?;
    let rs = parse(&out, checks);
    if rs.iter().all(|r| r.estado == Estado::Error) && !err.trim().is_empty() {
        return Err(err.trim().to_string());
    }
    Ok(rs)
}

/// Cuánto se espera antes de dar el equipo por perdido.
///
/// Más que el inventario: son veinte comandos en serie, y algunos —consultar la
/// política de auditoría, listar actualizaciones— tardan solos varios segundos.
pub const TIMEOUT_SECS: u64 = 240;

/// Pasa los checks en un equipo remoto. Se puede parar y tiene plazo.
pub fn run_remote(
    h: &crate::hosts::Host,
    password: &str,
    checks: &[Check],
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<Resultado>, String> {
    use std::sync::atomic::Ordering;
    let s = script(checks, h.protocol == crate::hosts::Protocol::Winrm);
    let (tx, rx) = std::sync::mpsc::channel();
    crate::hosts::run_remote_streaming(h, password, &s, &tx, stop, None, Some(TIMEOUT_SECS))?;
    let mut out = String::new();
    let mut err = String::new();
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
            crate::hosts::Line::Done(_) => {}
        }
    }
    if stop.load(Ordering::Relaxed) {
        return Err(format!(
            "La revisión de {} se detuvo antes de terminar (parada o sin respuesta en {}s).",
            h.name, TIMEOUT_SECS
        ));
    }
    let rs = parse(&out, checks);
    if rs.iter().all(|r| r.estado == Estado::Error) {
        let motivo = err.trim();
        return Err(if motivo.is_empty() {
            format!("{} no devolvió el resultado de ningún check.", h.name)
        } else {
            motivo.to_string()
        });
    }
    Ok(rs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chk(id: &str, espera: Espera) -> Check {
        Check {
            id: id.into(),
            title: "t".into(),
            category: "c".into(),
            severity: Severidad::High,
            command: "echo".into(),
            espera,
            remediation: "r".into(),
        }
    }

    fn linea(id: &str, ec: i32, ev: &str) -> String {
        format!("{MARCA}{US}{id}{US}{ec}{US}{ev}")
    }

    #[test]
    fn el_catalogo_compartido_se_lee_entero() {
        // Va por `include_str!` del MISMO fichero que usa la app: dos copias
        // acabarían discrepando en el check que alguien corrigió en un lado, y
        // las dos mitades del programa darían dos notas del mismo servidor.
        let w = catalogo(true);
        let l = catalogo(false);
        assert_eq!(w.len(), 20, "windows: {}", w.len());
        assert_eq!(l.len(), 20, "linux: {}", l.len());
        // Todos con comando y con forma de evaluarse — si uno se cae al leer, la
        // nota se calcula sobre diecinueve y nadie se entera.
        assert!(w.iter().all(|c| !c.command.is_empty()));
        assert!(w.iter().all(|c| !c.id.is_empty()));
        // Y las dos formas de `expect` están representadas: implementar solo la
        // del texto dejaba tres checks fallando siempre.
        assert!(w.iter().any(|c| matches!(c.espera, Espera::Salida(_))));
        assert!(l.iter().any(|c| matches!(c.espera, Espera::Salida(_))));
        assert!(w.iter().any(|c| matches!(c.espera, Espera::Contiene(_))));
    }

    #[test]
    fn un_check_que_no_se_pudo_ejecutar_no_cuenta_como_fallo() {
        // Decir «FALLA» afirma que el equipo está mal configurado cuando lo que
        // pasa es que no se pudo mirar. Y hunde la nota por un permiso.
        let cs = vec![
            chk("A", Espera::Contiene("PASS".into())),
            chk("B", Espera::Contiene("PASS".into())),
            chk("C", Espera::Contiene("PASS".into())),
        ];
        let salida = [
            linea("A", 0, "PASS"),
            linea("B", 0, "FAIL"),
            // Reventó: código distinto de cero y sin decidir nada.
            linea("C", 127, "auditpol: no se encuentra"),
        ]
        .join("\n");
        let rs = parse(&salida, &cs);
        assert_eq!(rs[0].estado, Estado::Pasa);
        assert_eq!(rs[1].estado, Estado::Falla);
        assert_eq!(rs[2].estado, Estado::Error);
        // La nota sale sobre los DOS que se pudieron medir, no sobre tres.
        assert_eq!(porcentaje(&rs), (50, 1));
    }

    #[test]
    fn una_salida_vacia_no_es_un_fallo_sino_un_check_que_no_decidio() {
        // ENCONTRADO EJECUTANDO ESTO CONTRA ESTA MÁQUINA. El check W1.1 hace
        // `net accounts | Select-String 'Length of password history' | …`, y en un
        // Windows en español `net accounts` dice «Duración del historial de
        // contraseñas»: la tubería no casa nada, no imprime nada, y el código de
        // salida es CERO porque nada ha reventado.
        //
        // Eso salía FALLA — el panel afirmaba que el equipo tiene mal la política
        // de contraseñas a partir de una medición que no existió. Tres de los
        // veinte caían así, y la nota los contaba.
        let cs = vec![chk("A", Espera::Contiene("PASS".into()))];
        assert_eq!(parse(&linea("A", 0, ""), &cs)[0].estado, Estado::Error);
        assert_eq!(parse(&linea("A", 0, "   "), &cs)[0].estado, Estado::Error);
        // Pero vacío con `Espera::Salida` sí decide: ahí el dato es el código.
        let cs2 = vec![chk("A", Espera::Salida(0))];
        assert_eq!(parse(&linea("A", 0, ""), &cs2)[0].estado, Estado::Pasa);
    }

    #[test]
    fn un_check_que_dice_fail_falla_aunque_el_codigo_no_sea_cero() {
        // Muchos checks acaban en `... && echo PASS || echo FAIL`, y el `||` deja
        // un código distinto de cero. Tratarlo como «no se pudo» escondería
        // fallos reales, que es peor que el problema que la regla evita.
        let cs = vec![chk("A", Espera::Contiene("PASS".into()))];
        let rs = parse(&linea("A", 1, "FAIL"), &cs);
        assert_eq!(rs[0].estado, Estado::Falla);
    }

    #[test]
    fn los_checks_que_no_volvieron_salen_como_no_medidos() {
        // Es el caso de una sesión cortada a media lista. Omitirlos calcularía la
        // nota sobre los que dio tiempo a correr: un notable sobre media
        // auditoría.
        let cs = vec![chk("A", Espera::Contiene("PASS".into())), chk("B", Espera::Contiene("PASS".into()))];
        let rs = parse(&linea("A", 0, "PASS"), &cs);
        assert_eq!(rs.len(), 2, "un check que no volvió no puede desaparecer");
        assert_eq!(rs[1].estado, Estado::Error);
        assert_eq!(porcentaje(&rs), (100, 1));
    }

    #[test]
    fn el_check_que_mira_el_codigo_de_salida_no_mira_el_texto() {
        let cs = vec![chk("A", Espera::Salida(0))];
        assert_eq!(parse(&linea("A", 0, "lo que sea"), &cs)[0].estado, Estado::Pasa);
        assert_eq!(parse(&linea("A", 3, "PASS PASS PASS"), &cs)[0].estado, Estado::Falla);
    }

    #[test]
    fn la_evidencia_llega_entera_y_con_sus_comillas() {
        // LA DIFERENCIA CON LA V2. Para proteger su JSON casero sustituye por
        // PUNTOS todas las comillas y barras invertidas de la salida, así que la
        // prueba de que una ruta está bien puesta llega como `.etc.ssh.config`.
        let cs = vec![chk("A", Espera::Contiene("PermitRootLogin".into()))];
        let original = "PermitRootLogin no\n# en \"/etc/ssh/sshd_config\"\nC:\\nueva\\ruta";
        // Tal y como lo escaparía el equipo remoto.
        let escapado = original.replace('\\', "\\\\").replace('\n', "\\n");
        let rs = parse(&linea("A", 0, &escapado), &cs);
        assert_eq!(rs[0].evidencia, original, "la evidencia no sobrevivió");
        assert_eq!(rs[0].estado, Estado::Pasa);
    }

    #[test]
    fn desescapar_no_convierte_una_ruta_de_windows_en_un_salto_de_linea() {
        // `replace("\\\\","\\")` seguido de `replace("\\n","\n")` convierte la
        // secuencia `\\n` —barra invertida literal más ene, que es lo que hay en
        // `C:\nueva`— en un salto. Por eso va de una pasada.
        assert_eq!(desescapa("C:\\\\nueva"), "C:\\nueva");
        assert_eq!(desescapa("una\\nlinea"), "una\nlinea");
        assert_eq!(desescapa("C:\\\\n"), "C:\\n");
        // Y una secuencia que no reconoce se deja como estaba.
        assert_eq!(desescapa("50\\%"), "50\\%");
    }

    #[test]
    fn la_ruta_de_un_check_no_puede_cerrar_su_comilla() {
        // El comando lo escribe el catálogo, pero el catálogo es un fichero que
        // se edita: un apóstrofo mal puesto no puede convertirse en ejecución.
        let cs = vec![Check { command: "echo 'a'; calc; echo '".into(), ..chk("A", Espera::Salida(0)) }];
        let w = script(&cs, true);
        assert_eq!(w.matches('\'').count() % 2, 0, "{w}");
        let l = script(&cs, false);
        assert!(l.contains("'\\''"), "escapado POSIX mal hecho: {l}");
    }

    #[test]
    fn el_script_de_linux_no_da_por_hecho_que_hay_bash() {
        // La V2 usa `bash -c`. En un Alpine eso hace que los veinte checks salgan
        // «no se pudo» y el panel diga que el contenedor está sin auditar.
        let l = script(&[chk("A", Espera::Salida(0))], false);
        assert!(l.contains("sh -c"), "{l}");
        assert!(!l.contains("bash -c"), "{l}");
    }

    #[test]
    fn un_equipo_sin_shell_no_se_audita() {
        let h = crate::hosts::Host {
            id: "h".into(),
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
        assert!(catalogo_de(&h).unwrap_err().contains("CACHE"));
    }

    #[test]
    fn la_gravedad_decide_si_un_fallo_es_falla_o_aviso() {
        // Sin esa separación la lista es inútil pasando de diez filas: «faltan
        // actualizaciones» y «SMBv1 sigue habilitado» salen del mismo color y con
        // la misma urgencia, así que se leen con la misma prisa — ninguna.
        let grave = Check { severity: Severidad::Critical, ..chk("A", Espera::Contiene("PASS".into())) };
        let leve = Check { severity: Severidad::Medium, ..chk("B", Espera::Contiene("PASS".into())) };
        let cs = vec![grave, leve];
        let salida = [linea("A", 0, "FAIL"), linea("B", 0, "FAIL")].join("\n");
        let rs = parse(&salida, &cs);
        assert_eq!(rs[0].estado, Estado::Falla);
        assert_eq!(rs[1].estado, Estado::Aviso);
        // Y un aviso NO cuenta como conforme: es un fallo, solo que menos urgente.
        assert!(!rs[1].estado.conforme());
        assert_eq!(porcentaje(&rs), (0, 0));
    }

    #[test]
    fn la_gravedad_ordena_lo_critico_arriba() {
        let mut v = vec![Severidad::Low, Severidad::Critical, Severidad::Medium, Severidad::High];
        v.sort();
        assert_eq!(v, vec![Severidad::Critical, Severidad::High, Severidad::Medium, Severidad::Low]);
    }

    #[test]
    fn sin_nada_medido_la_nota_es_cero_y_lo_dice() {
        // Y no una división por cero, ni un 100 % por vacuidad — que es lo que
        // daría un «todos los que se midieron pasaron» sobre cero medidos.
        let cs = vec![chk("A", Espera::Contiene("PASS".into()))];
        let rs = parse("", &cs);
        assert_eq!(porcentaje(&rs), (0, 1));
    }
}
