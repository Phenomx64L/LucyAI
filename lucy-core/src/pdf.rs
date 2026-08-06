//! Sacar el texto de un PDF.
//!
//! Es el extractor de ADJUNTOS —el que corre cuando alguien suelta un manual en
//! el compositor—, no el de ingesta: ése trocea, calcula embeddings y escribe
//! filas, y sigue viviendo en `commands/pdf.rs` con el resto de la ruta RAG.
//!
//! Vive aquí porque el shell nativo lo necesita y no puede llamar a un comando
//! de Tauri. La app real delega en esta función, así que un PDF adjuntado desde
//! el WebView y uno adjuntado desde egui producen el mismo texto — que era el
//! punto de tener un núcleo compartido.
//!
//! LA CADENA DE DOS. Primero `markitdown`, si está instalado: conserva la
//! ESTRUCTURA —títulos, tablas, listas— y con su complemento de OCR lee incluso
//! escaneos. Si no está, `pdf-extract`, que es Rust puro y por tanto siempre
//! está. El orden importa: el resultado del primero es mejor, el del segundo es
//! seguro.

/// Intenta convertir el PDF a Markdown con `markitdown`.
///
/// Devuelve `None` —y quien llama cae al extractor interno— cuando la variable
/// `LUCY_DISABLE_MARKITDOWN` está puesta, cuando el binario no está en el PATH,
/// cuando sale con código distinto de cero, o cuando no escribe nada.
///
/// SEGURIDAD: la ruta se pasa como UN argumento de argv, sin shell por medio.
/// Un nombre de fichero con `&` o comillas es un nombre de fichero, no un
/// comando.
fn try_markitdown(path: &str) -> Option<String> {
    if std::env::var("LUCY_DISABLE_MARKITDOWN").is_ok() {
        return None;
    }
    let out = std::process::Command::new("markitdown").arg(path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let md = String::from_utf8_lossy(&out.stdout);
    let trimmed = md.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// El texto de un PDF, o por qué no se pudo sacar.
///
/// UN PDF SIN CAPA DE TEXTO NO ES UN FALLO DEL PROGRAMA y el mensaje lo dice:
/// es un escaneo, y lo que hay que hacer es pasarle OCR. Devolver una cadena
/// vacía dejaría al operador viendo su adjunto en el compositor mientras el
/// modelo recibe nada — que es exactamente el fallo por el que la gente acabó
/// pegando rutas absolutas para pedirle a Lucy que leyera el fichero ella.
pub fn extract_text(path: &std::path::Path) -> Result<String, String> {
    let display = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("documento.pdf")
        .to_string();

    let raw = match try_markitdown(&path.to_string_lossy()) {
        Some(md) => md,
        None => pdf_extract::extract_text(path)
            .map_err(|e| format!("No se pudo extraer texto de '{display}': {e}"))?,
    };

    if raw.trim().is_empty() {
        return Err(format!(
            "'{display}' no tiene capa de texto (probablemente es un escaneo). \
             Aplica OCR, o instala `markitdown` con su complemento de OCR para leerlo."
        ));
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_fichero_que_no_es_pdf_falla_diciendo_cual() {
        // El nombre en el mensaje no es cosmética: con tres adjuntos, un error
        // sin nombre no dice cuál de los tres hay que quitar.
        let d = std::env::temp_dir().join("lucy_test_no_es_un_pdf.pdf");
        std::fs::write(&d, b"esto no es un PDF").unwrap();
        let e = extract_text(&d).unwrap_err();
        let _ = std::fs::remove_file(&d);
        assert!(e.contains("lucy_test_no_es_un_pdf.pdf"), "{e}");
    }

    #[test]
    fn un_fichero_que_no_existe_no_entra_en_panico() {
        assert!(extract_text(std::path::Path::new("no_existe_12345.pdf")).is_err());
    }
}
