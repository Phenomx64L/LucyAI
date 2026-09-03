//! Qué es un fichero adjunto y qué se puede hacer con él.
//!
//! Antes esto vivía en el shell nativo y sabía decir que no: una imagen se
//! aceptaba con la nota "necesita la ruta de visión del backend" y un PDF con
//! "se extrae en el backend". Las dos notas eran ciertas y las dos eran el
//! mismo agujero — el adjunto se veía en el compositor y al modelo no le
//! llegaba nada.
//!
//! Vive en el núcleo porque la decisión —esto es texto, esto es una imagen que
//! el modelo puede ver, esto no se puede mandar y por esto— no es de interfaz.
//! Lo que se queda en la interfaz es el chip: su icono, su aspa y dónde se
//! dibuja.

use crate::turns::Image;

/// Tope del contenido de un adjunto de texto.
///
/// Un log de 400 MB arrastrado a la ventana no puede tumbar el proceso. El
/// recorte es por CARACTERES y no por bytes porque es lo que se le manda al
/// modelo, y porque cortar UTF-8 a mitad de un carácter produce una cadena que
/// no es texto.
pub const MAX_CHARS: usize = 200_000;

/// Tope de una imagen, en bytes del fichero.
///
/// Anthropic rechaza por encima de 5 MB y los demás rondan lo mismo. Cortarlo
/// aquí convierte un HTTP 400 con un cuerpo ilegible —que llega treinta
/// segundos después de haber subido la imagen— en una nota junto al chip antes
/// de mandar nada.
pub const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Qué clase de fichero es.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Image,
    Pdf,
}

impl Kind {
    pub fn of(path: &std::path::Path) -> Self {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "tif" | "tiff" => Self::Image,
            "pdf" => Self::Pdf,
            _ => Self::Text,
        }
    }
}

/// El tipo MIME que se le declara al proveedor, si es uno que acepta.
///
/// La lista es corta A PROPÓSITO: los cuatro que las tres APIs aceptan. Un BMP
/// o un TIFF se reconocen como imagen —para poder explicar por qué no van— pero
/// no tienen tipo aquí. Mandarlos con un `image/bmp` inventado da un 400; y
/// mentir diciendo `image/png` da otro, más adelante y peor.
fn media_type(path: &std::path::Path) -> Option<&'static str> {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Un fichero adjunto a una orden, ya leído.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub name: String,
    /// De dónde salió, entera.
    ///
    /// EL NOMBRE SOLO NO BASTA, y lo que faltaba era esto. Al modelo se le
    /// anteponía «--- fichero adjunto: proyecto.xml ---» y el texto. Con eso
    /// puede LEERLO, pero no puede tocarlo: para proponer un `writefile` o un
    /// `editfile` necesita la ruta, y lo único que tenía era un nombre suelto.
    /// Así que escribía el nombre a secas —una ruta relativa— y el cambio se
    /// preparaba contra la carpeta de instalación.
    ///
    /// Vacía mientras el adjunto está pendiente y en los que no se pudieron
    /// leer, que es cuando no hay nada que proponer sobre ellos.
    pub path: String,
    pub kind: Kind,
    /// Lo que se antepone al prompt. El texto del fichero, o el del PDF ya
    /// extraído. Vacío para las imágenes.
    pub text: String,
    /// La imagen ya codificada, lista para el turno. `None` si no es una imagen
    /// o si no se pudo usar.
    pub image: Option<Image>,
    /// Por qué no se puede mandar, cuando no se puede. Vacío = se manda.
    pub blocked: String,
    /// Todavía se está leyendo.
    ///
    /// Existe por los PDF: extraer texto lanza `markitdown` —un subproceso de
    /// Python— y puede tardar decenas de segundos. Hacerlo en el hilo de la
    /// interfaz congelaría la ventana justo en la migración cuyo motivo era que
    /// la ventana no se congela. Así que el chip aparece en cuanto se suelta el
    /// fichero y se rellena cuando termina.
    ///
    /// No es lo mismo que `blocked`: bloqueado es "no va a poder mandarse",
    /// pendiente es "todavía no". Pintar el segundo como el primero haría que
    /// cada PDF pareciera un error durante los diez segundos que tarda.
    pub pending: bool,
}

impl Attachment {
    /// El hueco que se enseña mientras se lee de verdad, en otro hilo.
    pub fn pending(name: impl Into<String>, kind: Kind) -> Self {
        Self {
            name: name.into(),
            path: String::new(),
            kind,
            text: String::new(),
            image: None,
            blocked: String::new(),
            pending: true,
        }
    }

    /// Si puede viajar al modelo tal cual está.
    pub fn ready(&self) -> bool {
        self.blocked.is_empty() && !self.pending
    }

    /// El bloque que se antepone a la orden del operador.
    ///
    /// ESTÁ AQUÍ Y NO EN EL SHELL porque es lo que decide si el adjunto sirve, y
    /// eso merece un test. Era un `format!` de una línea metido en el bucle de
    /// enviar —«--- fichero adjunto: proyecto.xml ---» y el texto— y de esa
    /// línea salieron los dos fallos que se vieron usando Lucy de verdad:
    ///
    ///  · Lucy tenía el contenido delante y aun así pedía
    ///    `readfile:proyecto.xml` para comprobarlo. Con el nombre a secas, que
    ///    es una ruta relativa; fallaba, y concluía que no había logrado
    ///    procesar el fichero.
    ///
    ///  · Y al pedirle que lo corrigiera, el `writefile` salía con esa misma
    ///    ruta relativa, así que el cambio se preparaba contra la carpeta de
    ///    instalación en lugar de contra el fichero del operador.
    ///
    /// Las dos se cierran diciendo dos cosas que antes no se decían: dónde está,
    /// y que ya está leído.
    pub fn bloque_de_prompt(&self) -> String {
        if self.image.is_some() {
            // El modelo ve la imagen, pero no su nombre. Decírselo importa
            // cuando van tres: «en captura-2» es una frase que el operador puede
            // escribir y que, si no, no significa nada.
            return format!("--- imagen adjunta: {} ---\n", self.name);
        }
        format!(
            "--- fichero adjunto: {} ---\n\
             Ruta completa: {}\n\
             Su contenido va aquí debajo, ya leído: NO hace falta que lo pidas con readfile. \
             Si hay que cambiarlo, usa esa ruta completa.\n\n\
             {}\n\n",
            self.name,
            if self.path.is_empty() { "(desconocida)" } else { &self.path },
            self.text
        )
    }

    /// Lee un fichero del disco y decide qué se puede hacer con él.
    ///
    /// Un adjunto que no se va a poder mandar SE ACEPTA IGUAL y dice por qué.
    /// Rechazarlo en silencio al soltarlo deja al operador pensando que el
    /// arrastre no funciona; aceptarlo y mandarlo vacío es peor todavía.
    pub fn read(path: &std::path::Path) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("(sin nombre)")
            .to_string();
        let kind = Kind::of(path);
        let bloqueado = |por_que: String| Self {
            name: name.clone(),
            path: String::new(),
            kind,
            text: String::new(),
            image: None,
            blocked: por_que,
            pending: false,
        };

        // La ruta se pone AL FINAL y de una vez, y no en cada uno de los cinco
        // sitios donde se arma un `Self`. Con cinco copias, el que se olvide es
        // el que nadie prueba — y aquí el olvido no da error de compilación
        // cuando el campo ya existe.
        let mut leido = match kind {
            Kind::Image => {
                let Some(mt) = media_type(path) else {
                    return bloqueado(
                        "formato de imagen que los modelos no aceptan; conviértela a PNG o JPEG"
                            .into(),
                    );
                };
                // El tamaño se mira ANTES de leer: el sentido del tope es no
                // tener 40 MB en memoria, y comprobarlo después de cargarlos ya
                // los tuvo.
                match std::fs::metadata(path) {
                    Ok(m) if m.len() > MAX_IMAGE_BYTES => {
                        return bloqueado(format!(
                            "pesa {:.1} MB y el máximo son {} MB",
                            m.len() as f64 / 1_048_576.0,
                            MAX_IMAGE_BYTES / 1_048_576
                        ));
                    }
                    Err(e) => return bloqueado(format!("no se pudo abrir: {e}")),
                    _ => {}
                }
                match std::fs::read(path) {
                    Ok(bytes) => Self {
                        name,
                        // Se rellena al salir del `match`, en un solo sitio.
                        path: String::new(),
                        kind,
                        text: String::new(),
                        image: Some(Image { media_type: mt.into(), b64: b64_encode(&bytes) }),
                        blocked: String::new(),
                        pending: false,
                    },
                    Err(e) => bloqueado(format!("no se pudo leer: {e}")),
                }
            }
            Kind::Pdf => match crate::pdf::extract_text(path) {
                Ok(t) => Self {
                    name,
                    path: String::new(),
                    kind,
                    text: t.chars().take(MAX_CHARS).collect(),
                    image: None,
                    blocked: String::new(),
                    pending: false,
                },
                // El extractor ya explica el caso interesante —un escaneo sin
                // capa de texto— y lo dice mejor de lo que podría decirlo aquí.
                Err(e) => bloqueado(e),
            },
            Kind::Text => match std::fs::read_to_string(path) {
                Ok(s) => Self {
                    name,
                    path: String::new(),
                    kind,
                    text: s.chars().take(MAX_CHARS).collect(),
                    image: None,
                    blocked: String::new(),
                    pending: false,
                },
                // Un binario cualquiera cae aquí: no es UTF-8 y no hay nada
                // sensato que mandarle al modelo.
                Err(e) => bloqueado(format!("no se pudo leer como texto: {e}")),
            },
        };
        // Solo si se pudo leer. Un adjunto bloqueado no tiene nada que se pueda
        // proponer sobre él, y darle ruta invitaría al modelo a editar un
        // fichero cuyo contenido no ha visto.
        if leido.blocked.is_empty() {
            leido.path = path.display().to_string();
        }
        leido
    }
}

/// Base64 estándar, con relleno.
///
/// A mano y no con la caja `base64` para no meter una dependencia entera —con
/// su alfabeto configurable, sus motores y su decodificador— por veinte líneas
/// que solo codifican. Aquí nunca se decodifica: los bytes entran del disco y
/// salen hacia una petición HTTP.
pub fn b64_encode(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        // El relleno depende de cuántos bytes REALES tenía el trozo, no de los
        // ceros con los que se completó.
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}


/// Una imagen reducida, lista para subir a la GPU.
///
/// EN RGBA CRUDO y no en un `DynamicImage`: quien la recibe es el shell, y lo
/// unico que sabe hacer con esto es armar una textura. Devolver el tipo del
/// crate `image` obligaria al shell a depender de `image` para nada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Miniatura {
    pub ancho: u32,
    pub alto: u32,
    /// Cuatro bytes por pixel, sin padding.
    pub rgba: Vec<u8>,
}

/// El lado maximo de una miniatura, en pixeles.
///
/// ── POR QUE 160 Y NO EL TAMAÑO DE PANTALLA ───────────────────────────────────
///
/// La miniatura sube a la GPU como textura y se queda ahi mientras el mensaje
/// este en la conversacion. Una captura de 4K son 33 MB en RGBA; treinta
/// mensajes con captura serian un giga de memoria de video por una conversacion
/// de una tarde.
///
/// 160 es lo que se ve: un chip que cabe en la fila del compositor y se
/// reconoce de un vistazo. Quien quiera mirarla de verdad abre el fichero.
pub const LADO_MINIATURA: u32 = 160;

/// Reduce una imagen del disco. `None` si no es una imagen o no se pudo leer.
///
/// BLOQUEANTE: decodificar una captura de pantalla son decenas de milisegundos
/// y un frame son 16,7. Quien llama ya esta en un hilo.
///
/// SE MANTIENE LA PROPORCION. `thumbnail` de `image` encaja la imagen DENTRO del
/// cuadro sin deformarla, asi que una captura apaisada sale apaisada. Deformarla
/// para llenar un cuadrado haria irreconocible justo lo que la miniatura viene a
/// hacer reconocible.
pub fn miniatura(ruta: &std::path::Path, lado: u32) -> Option<Miniatura> {
    if Kind::of(ruta) != Kind::Image {
        return None;
    }
    let img = image::open(ruta).ok()?;
    let mini = img.thumbnail(lado, lado).to_rgba8();
    Some(Miniatura {
        ancho: mini.width(),
        alto: mini.height(),
        rgba: mini.into_raw(),
    })
}

#[cfg(test)]
// Igual que en `maintenance`, `suggest`, `clipboard` y `nexshell`: la asercion
// del lado de la miniatura compara CONSTANTES entre si. Clippy la ve evaluable en
// compilacion y avisa; es una guarda de invariante — fija que una miniatura siga
// siendo una miniatura, porque lo que la hace pequeña es que sube a la GPU.
#[allow(clippy::assertions_on_constants)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn un_adjunto_de_texto_llega_al_modelo_con_su_ruta_y_ya_leido() {
        // DE UN USO REAL. El operador arrastró un XML y pidió corregirlo. La
        // cabecera decía solo el nombre, así que Lucy pedía
        // `readfile:proyecto.xml` para «comprobarlo» —el nombre suelto es una
        // ruta relativa—, la lectura fallaba, y contestaba que no había logrado
        // procesar el fichero. Habiéndolo tenido delante todo el rato.
        let dir = std::env::temp_dir().join("lucy-adjunto-con-ruta");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("proyecto.xml");
        std::fs::write(&f, "<project/>").unwrap();

        let a = Attachment::read(&f);
        assert!(a.blocked.is_empty(), "no se pudo leer: {}", a.blocked);
        assert_eq!(a.path, f.display().to_string(), "el adjunto perdió su ruta");

        let b = a.bloque_de_prompt();
        // DÓNDE ESTÁ. Sin esto no hay `writefile` posible sobre él: lo único que
        // el modelo puede escribir es el nombre, que es relativo.
        assert!(b.contains(&f.display().to_string()), "el bloque no lleva la ruta: {b}");
        // Y QUE YA ESTÁ LEÍDO, para que no gaste un turno en ir a por él.
        assert!(b.contains("readfile"), "no se le dice que no hace falta releerlo: {b}");
        assert!(b.contains("<project/>"), "el bloque no lleva el contenido: {b}");

        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn uno_que_no_se_pudo_leer_no_lleva_ruta() {
        // Dar ruta a un adjunto bloqueado invita al modelo a proponer un cambio
        // sobre un fichero cuyo contenido no ha visto. El «antes» de ese diff
        // saldría de leerlo otra vez, no de lo que se le enseñó.
        let a = Attachment::read(Path::new("C:\\no-existe-esto-de-lucy.txt"));
        assert!(!a.blocked.is_empty(), "debería estar bloqueado");
        assert!(a.path.is_empty(), "un adjunto bloqueado llevó ruta: {}", a.path);
    }

    #[test]
    fn el_base64_coincide_con_los_vectores_del_rfc_4648() {
        // Los seis del RFC. El relleno es donde fallan las implementaciones
        // caseras, y es justo lo que un proveedor rechaza sin decir por qué.
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"f"), "Zg==");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_encode(b"foo"), "Zm9v");
        assert_eq!(b64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(b64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn el_base64_cubre_los_256_bytes() {
        // Un PNG tiene bytes altos en la primera línea. Un alfabeto mal escrito
        // se nota exactamente ahí y en ningún caso de prueba con texto ASCII.
        let todos: Vec<u8> = (0u8..=255).collect();
        let s = b64_encode(&todos);
        assert_eq!(s.len(), 344);
        assert!(s.starts_with("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"), "{s}");
        assert!(s.ends_with("8PHy8/T19vf4+fr7/P3+/w=="), "{s}");
    }

    #[test]
    fn la_clase_sale_de_la_extension() {
        assert_eq!(Kind::of(Path::new("captura.PNG")), Kind::Image);
        assert_eq!(Kind::of(Path::new("informe.pdf")), Kind::Pdf);
        assert_eq!(Kind::of(Path::new("lucy_app.log")), Kind::Text);
        // Sin extensión se asume texto: un `Dockerfile` o un `.env` son texto,
        // y equivocarse hacia texto solo cuesta un aviso de lectura fallida.
        assert_eq!(Kind::of(Path::new("Dockerfile")), Kind::Text);
    }

    #[test]
    fn un_bmp_se_reconoce_como_imagen_y_se_bloquea_con_motivo() {
        // Reconocerlo como imagen es lo que permite explicarlo. Si cayera en
        // Text, el error sería "no se pudo leer como texto" — cierto y sin
        // ninguna pista de qué hacer.
        assert_eq!(Kind::of(Path::new("captura.bmp")), Kind::Image);
        let a = Attachment::read(Path::new("captura.bmp"));
        assert!(a.blocked.contains("PNG"), "{}", a.blocked);
    }

    #[test]
    fn una_imagen_demasiado_grande_lo_dice_antes_de_mandarla() {
        let p = std::env::temp_dir().join("lucy_test_enorme.png");
        std::fs::write(&p, vec![0u8; (MAX_IMAGE_BYTES + 1) as usize]).unwrap();
        let a = Attachment::read(&p);
        let _ = std::fs::remove_file(&p);
        assert!(a.image.is_none());
        assert!(a.blocked.contains("MB"), "{}", a.blocked);
    }

    #[test]
    fn una_imagen_normal_sale_codificada_y_con_su_tipo() {
        let p = std::env::temp_dir().join("lucy_test_ok.jpg");
        std::fs::write(&p, b"foobar").unwrap();
        let a = Attachment::read(&p);
        let _ = std::fs::remove_file(&p);
        assert!(a.blocked.is_empty(), "{}", a.blocked);
        let img = a.image.expect("debería haber imagen");
        // jpg y jpeg son el mismo tipo MIME; declarar `image/jpg` da un 400.
        assert_eq!(img.media_type, "image/jpeg");
        assert_eq!(img.b64, "Zm9vYmFy");
    }

    #[test]
    fn un_texto_enorme_se_recorta_por_caracteres() {
        let p = std::env::temp_dir().join("lucy_test_log_grande.txt");
        // Acentos: si el recorte fuera por bytes, cortaría a mitad de carácter.
        std::fs::write(&p, "á".repeat(MAX_CHARS + 500)).unwrap();
        let a = Attachment::read(&p);
        let _ = std::fs::remove_file(&p);
        assert_eq!(a.text.chars().count(), MAX_CHARS);
        assert!(a.blocked.is_empty());
    }

    #[test]
    fn pendiente_no_es_lo_mismo_que_bloqueado() {
        // Los dos impiden mandarlo y solo uno es un error. Confundirlos haría
        // que cada PDF se pintara en ámbar —como un fallo— durante los diez
        // segundos que tarda en extraerse.
        let p = Attachment::pending("manual.pdf", Kind::Pdf);
        assert!(!p.ready(), "pendiente todavía no se puede mandar");
        assert!(p.blocked.is_empty(), "pendiente no es un motivo de rechazo");

        let b = Attachment::read(Path::new("captura.bmp"));
        assert!(!b.ready());
        assert!(!b.blocked.is_empty());
        assert!(!b.pending);
    }

    #[test]
    fn un_fichero_que_no_existe_se_acepta_diciendo_por_que_no_va() {
        // Aceptarlo y explicarlo, no descartarlo en silencio: soltar algo y que
        // no pase nada se lee como que el arrastre está roto.
        let a = Attachment::read(Path::new("no-existe-este-fichero.txt"));
        assert!(!a.blocked.is_empty());
        assert_eq!(a.name, "no-existe-este-fichero.txt");
    }

    #[test]
    fn una_miniatura_cabe_en_su_cuadro_y_no_se_deforma() {
        // Contra un PNG de verdad: el icono del propio proyecto, que es cuadrado
        // de 256. Si esto se cae, o el crate `image` perdio el formato o la
        // ruta cambio — las dos cosas hay que saberlas.
        let icono = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../lucy-svelte/icon.png");
        if !icono.exists() {
            // Sin el repositorio unificado al lado no hay contra que medir.
            return;
        }
        let m = miniatura(&icono, LADO_MINIATURA).expect("no decodifico el PNG");
        assert!(m.ancho <= LADO_MINIATURA && m.alto <= LADO_MINIATURA, "{}x{}", m.ancho, m.alto);
        assert!(m.ancho.max(m.alto) == LADO_MINIATURA, "no lleno el cuadro por el lado largo");
        assert_eq!(
            m.rgba.len(),
            (m.ancho * m.alto * 4) as usize,
            "el buffer no cuadra con las dimensiones"
        );
    }

    #[test]
    fn lo_que_no_es_una_imagen_no_da_miniatura() {
        // Un `.txt` o un PDF no tienen miniatura, y pedirsela al decodificador
        // seria leer megabytes para fallar.
        let txt = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(miniatura(&txt, LADO_MINIATURA).is_none());
        assert!(miniatura(std::path::Path::new("no-existe.png"), LADO_MINIATURA).is_none());
    }

    #[test]
    fn el_lado_esta_acotado_porque_esto_sube_a_la_gpu() {
        // Una captura de 4K son 33 MB en RGBA. Treinta mensajes con captura
        // serian un giga de memoria de video por una conversacion de una tarde.
        assert!(LADO_MINIATURA <= 320, "una miniatura de este tamaño no es una miniatura");
        assert!(LADO_MINIATURA >= 64, "por debajo de esto no se reconoce nada");
    }
}
