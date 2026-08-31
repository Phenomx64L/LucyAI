//! El idioma de la interfaz.
//!
//! CINCO IDIOMAS Y NO DOS, que son los que ofrece el instalador de la V1
//! (`SetupOverlay.svelte`: `t(es, pt, en, fr, de)`). El cockpit de la V2 se
//! quedó en español e inglés, y eso convierte la elección del instalador en una
//! promesa a medias: alguien que instala en portugués se encuentra media
//! aplicación en español. Aquí se cubren los cinco o no se cubre ninguno.
//!
//! LA CLAVE ES EL PROPIO TEXTO EN ESPAÑOL. Aquí hubo primero claves inventadas
//! —`nav.dashboard`, `cfg.tema`— y se cambió después de contar lo que hay que
//! convertir: unas trescientas cadenas repartidas por diecisiete mil líneas.
//!
//! Con clave inventada hay que reescribir cada sitio de llamada, inventar y
//! recordar trescientos identificadores, y una clave mal escrita pinta «‹falta›»
//! en pantalla. Con el español como clave, el sitio de llamada solo se ENVUELVE
//! —`tr("Modo privacidad")`— y lo que todavía no esté traducido cae al español,
//! QUE YA ES LA RESPUESTA CORRECTA para media plantilla. Esa propiedad es la que
//! hace viable convertir la aplicación por pantallas: una a medias se ve
//! mezclada, no rota.
//!
//! LOS TEXTOS VAN EN UN ARRAY, NO EN CAMPOS CON NOMBRE. Es la parte que hace que
//! esto no se pudra: añadir un idioma cambia el tamaño del array, y entonces el
//! compilador obliga a rellenarlo en TODAS las frases. Con campos con nombre, un
//! idioma nuevo entraría con `Default` vacío y media interfaz saldría en blanco
//! sin que nada avisara.

use std::sync::atomic::{AtomicU8, Ordering};

/// Los idiomas que ofrece Lucy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
    Pt,
    Fr,
    De,
}

impl Lang {
    /// Cuántos hay. El tamaño del array de cada frase sale de aquí.
    pub const N: usize = 5;

    /// EL ESPAÑOL EL PRIMERO, y el resto en el orden de `otros`. Cambiarlo sin
    /// cambiar las tablas dejaría cada idioma enseñando el texto de otro.
    pub const ALL: [Lang; Self::N] = [Lang::Es, Lang::En, Lang::Pt, Lang::Fr, Lang::De];

    /// Su posición. El español es el 0 y no tiene columna en `otros`.
    pub fn idx(self) -> usize {
        match self {
            Lang::Es => 0,
            Lang::En => 1,
            Lang::Pt => 2,
            Lang::Fr => 3,
            Lang::De => 4,
        }
    }

    /// El nombre del idioma EN ESE IDIOMA.
    ///
    /// «Deutsch» y no «Alemán»: quien busca su idioma en una lista lo busca como
    /// lo llama él, y si la interfaz está ahora mismo en un idioma que no
    /// entiende, un nombre traducido no le sirve para encontrarlo.
    pub fn nombre(self) -> &'static str {
        match self {
            Lang::Es => "Español",
            Lang::En => "English",
            Lang::Pt => "Português",
            Lang::Fr => "Français",
            Lang::De => "Deutsch",
        }
    }

    /// El código que guarda la V1 en `lucy_user_lang`.
    ///
    /// EL MISMO QUE LA V1 para que la elección se pueda llevar de una versión a
    /// otra: quien ya eligió portugués en la aplicación de escritorio no debería
    /// tener que volver a elegirlo aquí.
    pub fn clave(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
            Lang::Pt => "pt",
            Lang::Fr => "fr",
            Lang::De => "de",
        }
    }

    /// Del código guardado. Español si no se sabe.
    ///
    /// POR PREFIJO, porque lo que se guarda es `es-MX` o `en-US`, no `es`. Un
    /// `==` exacto haría que un `es-MX` guardado por la V1 se leyera como «no lo
    /// sé» y volviera al idioma por defecto — que casualmente es el mismo, así
    /// que el fallo no se vería hasta que alguien eligiera `pt-BR`.
    pub fn de_clave(v: &str) -> Option<Lang> {
        let v = v.trim().to_ascii_lowercase();
        Lang::ALL.into_iter().find(|l| v.starts_with(l.clave()))
    }
}

/// El idioma puesto ahora mismo, como índice.
///
/// ATÓMICO Y GLOBAL como `motion()`, y por la misma razón: lo lee cada texto de
/// cada frame desde sitios que no tienen acceso a la aplicación, y también el
/// hilo que construye el prompt. Pasarlo por parámetro obligaría a llevarlo por
/// toda la pila de dibujo.
static ACTUAL: AtomicU8 = AtomicU8::new(0);

/// El idioma puesto.
pub fn lang() -> Lang {
    let i = ACTUAL.load(Ordering::Relaxed) as usize;
    Lang::ALL.get(i).copied().unwrap_or(Lang::Es)
}

/// Cambia el idioma. Tiene efecto en el frame siguiente.
pub fn set(l: Lang) {
    ACTUAL.store(l.idx() as u8, Ordering::Relaxed);
}

/// Una frase y sus traducciones.
pub struct Frase {
    /// El texto EN ESPAÑOL, que es a la vez la clave. Ver la cabecera.
    pub es: &'static str,
    /// Las otras cuatro, en el orden de [`Lang::ALL`] sin el español.
    pub otros: [&'static str; Lang::N - 1],
}

/// El texto en el idioma puesto, a partir del español.
///
/// DEVUELVE EL ESPAÑOL si no hay traducción, sin quejarse. No es una rendición:
/// es lo que permite convertir la aplicación por pantallas sin que las que
/// faltan se vean rotas.
///
/// NO PIDE `&'static str`, y eso es lo que hace que esto sea abordable. Con
/// `'static` habría que envolver los trescientos sitios de llamada uno a uno;
/// aceptando cualquier vida, la traducción puede hacerla el AYUDANTE que ya
/// pinta el texto —`fila`, `panel`, `segmentado`, `insignia`— y entonces media
/// pantalla queda traducida con seis cambios en vez de con ciento cincuenta.
///
/// El truco de la firma: si hay traducción se devuelve la `'static` de la tabla,
/// que vale como `'a`; si no, se devuelve la entrada. Las dos encajan.
pub fn tr<'a>(es: &'a str) -> &'a str {
    let l = lang();
    if l == Lang::Es {
        return es;
    }
    match busca(es) {
        Some(f) => {
            let s = f.otros[l.idx() - 1];
            if s.is_empty() {
                es
            } else {
                s
            }
        }
        None => es,
    }
}

/// Traduce una plantilla y rellena sus huecos.
///
/// LOS HUECOS VAN CON NOMBRE, `{n}` y no `{}`, y ésa es toda la razón de que
/// esto exista en vez de un `format!`. El hueco CAMBIA DE SITIO al traducir:
/// «hace {n} días» es «vor {n} Tagen» en alemán y «{n} days ago» en inglés, y
/// con huecos posicionales la traducción no puede reordenar sin que el valor
/// acabe en el sitio equivocado. Con nombre, la frase se escribe en cada idioma
/// como se dice en ese idioma.
///
/// Los valores llegan YA FORMATEADOS. Un `{:.1}` no sobrevive a una tabla de
/// traducción —nadie va a mantener el número de decimales en cinco idiomas— así
/// que el redondeo se decide donde se conoce el dato y aquí solo se pega texto.
///
/// Un hueco que la traducción se haya dejado NO se inventa: se queda sin
/// rellenar y se ve. Es feo a propósito — el test `las_plantillas_conservan_sus_huecos`
/// lo impide antes de llegar aquí, y si algo se colara, un «hace días» sin
/// número es más honesto que un número puesto donde parezca.
pub fn trf(plantilla: &'static str, pares: &[(&str, &str)]) -> String {
    let mut s = tr(plantilla).to_string();
    for (k, v) in pares {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// El nombre de un modelo, con su descripción traducida y su marca intacta.
///
/// EL CATÁLOGO GUARDA `Nombre — Descripción` EN UNA SOLA CADENA: «Gemini 3.5
/// Flash — Rendimiento de frontera sostenido». Meter eso entero en la tabla
/// significaría cuarenta y ocho entradas cuya mitad izquierda es una marca
/// registrada que un traductor puede tocar sin querer — y «Gemini 3.5 Blitz» en
/// el selector de modelo es un fallo que nadie sabría explicar.
///
/// Se parte por la raya y se traduce SOLO LA DERECHA. Treinta y seis
/// descripciones en vez de cuarenta y ocho nombres completos, y la parte que
/// identifica el modelo no pasa por ninguna traducción.
///
/// Sin raya —los ids de Ollama, que son `mistral:latest` a secas— se devuelve
/// tal cual: no hay descripción que traducir.
pub fn modelo(nombre: &str) -> String {
    match nombre.split_once(" — ") {
        Some((marca, desc)) => format!("{marca} — {}", tr(desc)),
        None => nombre.to_string(),
    }
}

/// Los nombres de los huecos de una plantilla, en orden de aparición.
///
/// Público porque lo usa el test que compara plantilla y traducciones, y ese
/// test es la mitad del valor de este mecanismo.
///
/// Y SOLO LO USA ESE TEST, así que en la compilación normal está muerta y el
/// compilador lo dice. Se silencia AQUÍ y con el motivo, en vez de dejar que el
/// aviso salga en cada `cargo build`: un aviso que sale siempre y que todo el
/// mundo sabe que no importa es un sitio donde el siguiente aviso —el que sí
/// importa— pasa desapercibido. Se comprobó antes de callarlo: el test es
/// `las_plantillas_conservan_sus_huecos_en_los_cinco_idiomas`, y lo que vigila
/// es que una traducción no se deje un `{n}` por el camino, que es un valor que
/// desaparece de la frase sin que falle nada.
#[allow(dead_code)]
pub fn huecos(s: &str) -> Vec<&str> {
    let mut v = Vec::new();
    let mut resto = s;
    while let Some(a) = resto.find('{') {
        let Some(b) = resto[a..].find('}') else { break };
        let nombre = &resto[a + 1..a + b];
        // `{{` escapado, o un hueco vacío: ni uno ni otro son un nombre.
        if !nombre.is_empty() && nombre.chars().all(|c| c.is_alphanumeric() || c == '_') {
            v.push(nombre);
        }
        resto = &resto[a + b + 1..];
    }
    v
}

/// La frase de un texto español, si está.
///
/// BÚSQUEDA BINARIA sobre la tabla ordenada. Con cientos de frases y una pantalla
/// que las pide todas sesenta veces por segundo, un recorrido lineal serían
/// decenas de miles de comparaciones por frame. El test de abajo comprueba que
/// la tabla está ordenada, que es lo que hace válida la búsqueda.
fn busca(es: &str) -> Option<&'static Frase> {
    FRASES.binary_search_by(|f| f.es.cmp(es)).ok().map(|i| &FRASES[i])
}

/// Atajo para escribir la tabla sin repetir `Frase { es: …, otros: […] }`.
macro_rules! f {
    ($es:expr, $en:expr, $pt:expr, $fr:expr, $de:expr $(,)?) => {
        Frase { es: $es, otros: [$en, $pt, $fr, $de] }
    };
}

/// Todas las frases de la interfaz, ORDENADAS POR EL TEXTO ESPAÑOL.
///
/// El orden no es estético: lo exige la búsqueda binaria, y hay un test que lo
/// vigila y dice dónde se ha roto.
///
/// LOS NOMBRES PROPIOS NO SE TRADUCEN. NexShell, Log Viewer y Terminal IA son
/// partes de Lucy con nombre, no descripciones, así que no están en esta tabla:
/// `tr` devuelve el español, que es lo que se quiere. Que falten es la decisión,
/// no un olvido.
pub const FRASES: &[Frase] = &[
    // ── Visor de logs ───────────────────────────────────────────────────────
    f!(
        "\n\n_(1 imagen de este mensaje no se guardó al cerrar)_",
        "\n\n_(1 image from this message wasn't saved on close)_",
        "\n\n_(1 imagem desta mensagem não foi guardada ao fechar)_",
        "\n\n_(1 image de ce message n'a pas été enregistrée à la fermeture)_",
        "\n\n_(1 Bild dieser Nachricht wurde beim Schließen nicht gespeichert)_",
    ),
    f!(
        "\n\n_(detenido por el operador)_",
        "\n\n_(stopped by the operator)_",
        "\n\n_(parado pelo operador)_",
        "\n\n_(arrêté par l'opérateur)_",
        "\n\n_(vom Operator gestoppt)_",
    ),
    f!(
        "\n\n_({n} imágenes de este mensaje no se guardaron al cerrar)_",
        "\n\n_({n} images from this message weren't saved on close)_",
        "\n\n_({n} imagens desta mensagem não foram guardadas ao fechar)_",
        "\n\n_({n} images de ce message n'ont pas été enregistrées à la fermeture)_",
        "\n\n_({n} Bilder dieser Nachricht wurden beim Schließen nicht gespeichert)_",
    ),
    f!("  (desactivado)", "  (disabled)", "  (desativado)", "  (désactivé)", "  (deaktiviert)"),
    f!(
        "  _(sin migrar)_",
        "  _(not migrated)_",
        "  _(por migrar)_",
        "  _(non migré)_",
        "  _(nicht migriert)_",
    ),
    f!(
        "# Run de Lucy · {modelo}\n",
        "# Lucy run · {modelo}\n",
        "# Run da Lucy · {modelo}\n",
        "# Run de Lucy · {modelo}\n",
        "# Lucy-Run · {modelo}\n",
    ),
    f!(
        "#{id} · sesión {sesion} · {chars} caracteres leídos",
        "#{id} · session {sesion} · {chars} characters read",
        "#{id} · sessão {sesion} · {chars} caracteres lidos",
        "#{id} · session {sesion} · {chars} caractères lus",
        "#{id} · Sitzung {sesion} · {chars} Zeichen gelesen",
    ),
    f!(
        "(el comando no devolvió nada)",
        "(the command returned nothing)",
        "(o comando não devolveu nada)",
        "(la commande n'a rien renvoyé)",
        "(der Befehl gab nichts zurück)",
    ),
    f!(
        "(el comando terminó con error)",
        "(the command ended with an error)",
        "(o comando terminou com erro)",
        "(la commande s'est terminée en erreur)",
        "(der Befehl endete mit Fehler)",
    ),
    f!(
        "(equipo no encontrado)",
        "(machine not found)",
        "(máquina não encontrada)",
        "(machine introuvable)",
        "(Rechner nicht gefunden)",
    ),
    f!(
        "(sin cambios)",
        "(no changes)",
        "(sem alterações)",
        "(aucun changement)",
        "(keine Änderungen)",
    ),
    f!("(sin nombre)", "(unnamed)", "(sem nome)", "(sans nom)", "(ohne Namen)"),
    f!("(sin salida)", "(no output)", "(sem saída)", "(aucune sortie)", "(keine Ausgabe)"),
    f!(
        "**Lo que puedo hacer en este equipo**",
        "**What I can do on this machine**",
        "**O que posso fazer neste computador**",
        "**Ce que je peux faire sur ce poste**",
        "**Was ich auf diesem Rechner kann**",
    ),
    f!(
        "**Tope de gasto.** {motivo}",
        "**Spend limit.** {motivo}",
        "**Limite de gasto.** {motivo}",
        "**Limite de dépense.** {motivo}",
        "**Ausgabenlimit.** {motivo}",
    ),
    f!(
        "**{host}** · {os}\n\nCPU {cpu} % · RAM {usada} de {total} GB\n",
        "**{host}** · {os}\n\nCPU {cpu} % · RAM {usada} of {total} GB\n",
        "**{host}** · {os}\n\nCPU {cpu} % · RAM {usada} de {total} GB\n",
        "**{host}** · {os}\n\nCPU {cpu} % · RAM {usada} sur {total} Go\n",
        "**{host}** · {os}\n\nCPU {cpu} % · RAM {usada} von {total} GB\n",
    ),
    f!(
        "**{n} skills instalados**",
        "**{n} skills installed**",
        "**{n} skills instalados**",
        "**{n} skills installés**",
        "**{n} Skills installiert**",
    ),
    f!("+{n} más", "+{n} more", "+{n} mais", "+{n} autres", "+{n} weitere"),
    f!(
        "+{n} más — sigue escribiendo para acotar",
        "+{n} more — keep typing to narrow it down",
        "+{n} mais — continua a escrever para afinar",
        "+{n} de plus — continue d'écrire pour affiner",
        "+{n} weitere — tippe weiter zum Eingrenzen",
    ),
    f!(
        "- Ejecutar PowerShell, cmd, wmic, netsh, reg y cscript, con tu aprobación",
        "- Run PowerShell, cmd, wmic, netsh, reg and cscript, with your approval",
        "- Executar PowerShell, cmd, wmic, netsh, reg e cscript, com a tua aprovação",
        "- Exécuter PowerShell, cmd, wmic, netsh, reg et cscript, avec ton accord",
        "- PowerShell, cmd, wmic, netsh, reg und cscript ausführen, mit deiner Freigabe",
    ),
    f!(
        "1 comando propuesto — apruébalo en el panel de Plan",
        "1 command proposed — approve it in the Plan panel",
        "1 comando proposto — aprova-o no painel de Plano",
        "1 commande proposée — approuve-la dans le panneau Plan",
        "1 Befehl vorgeschlagen — gib ihn im Plan-Panel frei",
    ),
    f!(
        "1 paso sin aprobar caduca",
        "1 unapproved step expires",
        "1 passo por aprovar caduca",
        "1 étape non approuvée devient caduque",
        "1 nicht freigegebener Schritt wird hinfällig",
    ),
    f!(
        "1 patrón descartado — no volverá",
        "1 pattern discarded — it won't come back",
        "1 padrão descartado — não voltará",
        "1 motif écarté — il ne reviendra pas",
        "1 Muster verworfen — es kommt nicht zurück",
    ),
    f!("1 volumen", "1 volume", "1 volume", "1 volume", "1 Laufwerk"),
    f!(
        "192.168.1.10 ó servidor.empresa.local",
        "192.168.1.10 or server.company.local",
        "192.168.1.10 ou servidor.empresa.local",
        "192.168.1.10 ou serveur.entreprise.local",
        "192.168.1.10 oder server.firma.local",
    ),
    f!(
        "1M de contexto, menor costo",
        "1M context, lower cost",
        "1M de contexto, menor custo",
        "1M de contexte, coût réduit",
        "1M Kontext, geringere Kosten",
    ),
    f!("AVISOS", "WARNINGS", "AVISOS", "AVERTISSEMENTS", "WARNUNGEN"),
    f!(
        "Abre Lucy al menos una vez para crear la DB, o corre desde el mismo usuario.",
        "Open Lucy at least once to create the DB, or run as the same user.",
        "Abre a Lucy pelo menos uma vez para criar a DB, ou corre a partir do mesmo utilizador.",
        "Ouvre Lucy au moins une fois pour créer la DB, ou lance-la depuis le même utilisateur.",
        "Öffne Lucy mindestens einmal, um die DB anzulegen, oder starte als derselbe Benutzer.",
    ),
    f!("Activadas", "On", "Ativadas", "Activées", "Ein"),
    f!("Activado", "On", "Ativado", "Activé", "Ein"),
    f!("Activo", "On", "Ativo", "Actif", "Ein"),
    f!("Actualizado", "Updated", "Atualizado", "Mis à jour", "Aktualisiert"),
    f!(
        "Actualizar ahora",
        "Update now",
        "Atualizar agora",
        "Actualiser maintenant",
        "Jetzt aktualisieren",
    ),
    f!(
        "Adjuntar fichero — o arrastra uno a la ventana",
        "Attach file — or drag one onto the window",
        "Anexar ficheiro — ou arrasta um para a janela",
        "Joindre un fichier — ou glisse-en un dans la fenêtre",
        "Datei anhängen — oder zieh eine ins Fenster",
    ),
    f!(
        "Adjunto retenido: {nombre}",
        "Attachment held: {nombre}",
        "Anexo retido: {nombre}",
        "Pièce jointe retenue : {nombre}",
        "Anhang zurückgehalten: {nombre}",
    ),
    f!(
        "Agéntico y multimodal (más reciente)",
        "Agentic and multimodal (latest)",
        "Agêntico e multimodal (mais recente)",
        "Agentique et multimodal (le plus récent)",
        "Agentisch und multimodal (neuestes)",
    ),
    f!(
        "Ahora mismo resuelve a",
        "Currently resolves to",
        "Agora resolve para",
        "Actuellement résolu en",
        "Aktuell aufgelöst als",
    ),
    f!(
        "Alto (2× el costo de Opus 5)",
        "High (2× Opus 5 cost)",
        "Alto (2× o custo do Opus 5)",
        "Élevé (2× le coût d'Opus 5)",
        "Hoch (2× Kosten von Opus 5)",
    ),
    f!(
        "Alto (generación anterior)",
        "High (previous generation)",
        "Alto (geração anterior)",
        "Élevé (génération précédente)",
        "Hoch (Vorgängergeneration)",
    ),
    f!(
        "Alto (predeterminado)",
        "High (default)",
        "Alto (predefinido)",
        "Élevé (par défaut)",
        "Hoch (Standard)",
    ),
    f!(
        "Alto rendimiento",
        "High performance",
        "Alto desempenho",
        "Hautes performances",
        "Hohe Leistung",
    ),
    f!("Animaciones", "Animations", "Animações", "Animations", "Animationen"),
    f!("Apagadas", "Off", "Desligadas", "Désactivées", "Aus"),
    f!("Apagado", "Off", "Desligado", "Désactivé", "Aus"),
    f!("Aplicar", "Apply", "Aplicar", "Appliquer", "Übernehmen"),
    f!(
        "Aplicar el cambio en {ruta}",
        "Apply the change to {ruta}",
        "Aplicar a alteração em {ruta}",
        "Appliquer la modification dans {ruta}",
        "Änderung in {ruta} übernehmen",
    ),
    f!("Aprobar", "Approve", "Aprovar", "Approuver", "Genehmigen"),
    f!(
        "Apruébalo en Artefactos y quedará instalado.",
        "Approve it in Artifacts and it will be installed.",
        "Aprova-o em Artefactos e fica instalado.",
        "Approuve-le dans Artefacts et il sera installé.",
        "Gib es unter Artefakte frei, dann ist es installiert.",
    ),
    f!("Aquí estamos", "Here we are", "Aqui estamos", "On y est", "Da sind wir"),
    f!("Archivo", "Archive", "Arquivo", "Fichiers", "Archiv"),
    f!(
        "Arrancamos",
        "Getting started",
        "Arrancamos",
        "On démarre",
        "Los geht’s",
    ),
    f!("Artefactos", "Artifacts", "Artefactos", "Artefacts", "Artefakte"),
    f!("Atención", "Warning", "Atenção", "Attention", "Achtung"),
    f!("Auditoría", "Audit", "Auditoria", "Audit", "Audit"),
    f!(
        "Auto-introspección: skills, MCPs, frameworks",
        "Self-introspection: skills, MCPs, frameworks",
        "Auto-introspeção: skills, MCPs, frameworks",
        "Auto-introspection : skills, MCPs, frameworks",
        "Selbstintrospektion: Skills, MCPs, Frameworks",
    ),
    f!(
        "Automático apagado — cada comando lo apruebas tú. Encendido, Lucy encadena hasta {max} pasos sola.",
        "Auto mode off — you approve every command. Turned on, Lucy chains up to {max} steps on her own.",
        "Automático desligado — cada comando aprova-lo tu. Ligado, a Lucy encadeia até {max} passos sozinha.",
        "Mode auto désactivé — tu approuves chaque commande. Activé, Lucy enchaîne jusqu’à {max} étapes seule.",
        "Automatik aus — du genehmigst jeden Befehl. Eingeschaltet verkettet Lucy bis zu {max} Schritte allein.",
    ),
    f!(
        "Automático en pausa",
        "Auto mode paused",
        "Automático em pausa",
        "Automatique en pause",
        "Automatik pausiert",
    ),
    f!(
        "Automático encendido — {usados} de {max} pasos usados. Lucy ejecuta sola los comandos que el guardrail deja pasar. Se para en los que no.",
        "Auto mode on — {usados} of {max} steps used. Lucy runs the commands the guardrail lets through on her own. She stops at the ones it does not.",
        "Automático ligado — {usados} de {max} passos usados. A Lucy executa sozinha os comandos que o guardrail deixa passar. Para nos que não.",
        "Mode auto activé — {usados} sur {max} étapes utilisées. Lucy exécute seule les commandes que le garde-fou laisse passer. Elle s’arrête sur les autres.",
        "Automatik an — {usados} von {max} Schritten verbraucht. Lucy führt die Befehle, die das Guardrail durchlässt, allein aus. Bei den anderen hält sie an.",
    ),
    f!(
        "Avisar si el modelo se queda corto",
        "Warn if the model falls short",
        "Avisar se o modelo ficar aquém",
        "Prévenir si le modèle est trop juste",
        "Warnen, wenn das Modell nicht reicht",
    ),
    f!("Aviso", "Warning", "Aviso", "Avertissement", "Warnung"),
    f!("Avisos", "Warnings", "Avisos", "Avertissements", "Warnungen"),
    f!(
        "Avisos del vigilante que no has marcado como leídos. Se gestionan en Configuración.",
        "Watcher alerts you have not marked as read. They are managed in Settings.",
        "Avisos do vigilante que não marcaste como lidos. Gerem-se em Configuração.",
        "Alertes de la sentinelle que tu n'as pas marquées comme lues. Elles se gèrent dans Configuration.",
        "Hinweise des Wächters, die du nicht als gelesen markiert hast. Verwaltet in den Einstellungen.",
    ),
    f!("Avisos sin leer", "Unread alerts", "Avisos por ler", "Alertes non lues", "Ungelesene Hinweise"),
    f!(
        "Añadir el primero",
        "Add the first one",
        "Adicionar o primeiro",
        "Ajouter la première",
        "Ersten hinzufügen",
    ),
    f!(
        "Añadir equipo",
        "Add machine",
        "Adicionar máquina",
        "Ajouter une machine",
        "Rechner hinzufügen",
    ),
    f!(
        "Bajo (sensible a latencia)",
        "Low (latency-sensitive)",
        "Baixo (sensível à latência)",
        "Faible (sensible à la latence)",
        "Niedrig (latenzsensibel)",
    ),
    f!("Base de datos", "Database", "Base de dados", "Base de données", "Datenbank"),
    f!(
        "Bloqueado por el guardrail",
        "Blocked by the guardrail",
        "Bloqueado pelo guardrail",
        "Bloqué par le guardrail",
        "Vom Guardrail blockiert",
    ),
    f!(
        "Bloqueado por el guardrail: {motivo}",
        "Blocked by the guardrail: {motivo}",
        "Bloqueado pelo guardrail: {motivo}",
        "Bloqué par le guardrail : {motivo}",
        "Vom Guardrail blockiert: {motivo}",
    ),
    f!("Buen día", "Good day", "Boa tarde", "Bonne journée", "Guten Tag"),
    f!(
        "Buena noche",
        "Evening",
        "Boa noite",
        "Bonne soirée",
        "Schönen Abend",
    ),
    f!(
        "Buena tarde",
        "Afternoon",
        "Boa tarde",
        "Bel après-midi",
        "Schönen Nachmittag",
    ),
    f!(
        "Buenas noches",
        "Good evening",
        "Boa noite",
        "Bonsoir",
        "Guten Abend",
    ),
    f!(
        "Buenas tardes",
        "Good afternoon",
        "Boa tarde",
        "Bon après-midi",
        "Guten Tag",
    ),
    f!("Buenos días", "Good morning", "Bom dia", "Bonjour", "Guten Morgen"),
    f!("Buscando…", "Searching…", "A procurar…", "Recherche…", "Suche läuft…"),
    f!(
        "Buscar duplicados",
        "Find duplicates",
        "Procurar duplicados",
        "Chercher les doublons",
        "Duplikate suchen",
    ),
    f!(
        "Buscar modelo…",
        "Search models…",
        "Procurar modelo…",
        "Chercher un modèle…",
        "Modell suchen…",
    ),
    f!(
        "Bypass del fork advisor (esta pestaña)",
        "Bypass the fork advisor (this tab)",
        "Bypass do fork advisor (este separador)",
        "Bypass du fork advisor (cet onglet)",
        "Bypass des Fork Advisors (dieser Tab)",
    ),
    f!(
        "C:\\ruta\\a\\tu\\carpeta",
        "C:\\path\\to\\your\\folder",
        "C:\\caminho\\para\\a\\sua\\pasta",
        "C:\\chemin\\vers\\votre\\dossier",
        "C:\\Pfad\\zu\\Ihrem\\Ordner",
    ),
    f!(
        "C:\\ruta\\al\\archivo.log",
        "C:\\path\\to\\file.log",
        "C:\\caminho\\para\\ficheiro.log",
        "C:\\chemin\\vers\\fichier.log",
        "C:\\Pfad\\zur\\Datei.log",
    ),
    f!("CONFORMES", "COMPLIANT", "CONFORMES", "CONFORMES", "ERFÜLLT"),
    f!("CPU al {pct}%", "CPU at {pct}%", "CPU a {pct}%", "CPU à {pct} %", "CPU bei {pct}%"),
    f!("CPU alta ({pct}%)", "High CPU ({pct}%)", "CPU alta ({pct}%)", "CPU élevé ({pct}%)", "Hohe CPU-Last ({pct}%)"),
    f!("CPU · aviso", "CPU · warning", "CPU · aviso", "CPU · avertissement", "CPU · Warnung"),
    f!("CPU · crítico", "CPU · critical", "CPU · crítico", "CPU · critique", "CPU · kritisch"),
    f!(
        "CPU, RAM y red. Los servicios tienen su propia hora.",
        "CPU, RAM and network. Services have their own timestamp.",
        "CPU, RAM e rede. Os serviços têm a sua própria hora.",
        "CPU, RAM et réseau. Les services ont leur propre heure.",
        "CPU, RAM und Netz. Dienste haben ihren eigenen Zeitstempel.",
    ),
    f!(
        "Cada cristal es una sesión destilada. Se escriben solos al cerrar turnos; sus lecciones ya son memorias y sobreviven aunque borres el cristal.",
        "Each crystal is a distilled session. They write themselves when turns close; their lessons are already memories and survive even if you delete the crystal.",
        "Cada cristal é uma sessão destilada. Escrevem-se sozinhos ao fechar turnos; as suas lições já são memórias e sobrevivem mesmo que apagues o cristal.",
        "Chaque cristal est une session distillée. Ils s'écrivent seuls à la fin des tours ; leurs leçons sont déjà des mémoires et survivent même si tu supprimes le cristal.",
        "Jeder Kristall ist eine destillierte Sitzung. Sie schreiben sich selbst, wenn Runden enden; ihre Lehren sind bereits Erinnerungen und bleiben, auch wenn du den Kristall löschst.",
    ),
    f!(
        "Cadena detenida por un fallo del proveedor",
        "Chain stopped by a provider failure",
        "Cadeia interrompida por uma falha do fornecedor",
        "Chaîne arrêtée par un échec du fournisseur",
        "Kette wegen Anbieterfehler gestoppt",
    ),
    f!(
        "Caducado — llegó una orden nueva",
        "Expired — a new instruction arrived",
        "Caducado — chegou uma ordem nova",
        "Périmé — un nouvel ordre est arrivé",
        "Hinfällig — es kam ein neuer Befehl",
    ),
    f!(
        "Cambiar el modelo activo",
        "Change the active model",
        "Mudar o modelo ativo",
        "Changer le modèle actif",
        "Aktives Modell wechseln",
    ),
    f!(
        "Cambiar el tema visual",
        "Change the visual theme",
        "Mudar o tema visual",
        "Changer le thème visuel",
        "Design wechseln",
    ),
    f!(
        "Cancelado — el operador detuvo la respuesta",
        "Canceled — the operator stopped the response",
        "Cancelado — o operador parou a resposta",
        "Annulé — l'opérateur a arrêté la réponse",
        "Abgebrochen — Antwort vom Operator gestoppt",
    ),
    f!(
        "Cancelado — falló el turno que lo propuso",
        "Canceled — the turn that proposed it failed",
        "Cancelado — falhou o turno que o propôs",
        "Annulé — échec du tour qui l'a proposé",
        "Abgebrochen — der vorschlagende Turn ist fehlgeschlagen",
    ),
    f!("Cancelar", "Cancel", "Cancelar", "Annuler", "Abbrechen"),
    f!(
        "Capturar snapshot del sistema",
        "Capture a system snapshot",
        "Capturar snapshot do sistema",
        "Capturer un snapshot du système",
        "System-Snapshot aufnehmen",
    ),
    f!("Cargando…", "Loading…", "A carregar…", "Chargement…", "Lädt…"),
    f!(
        "Cargas sensibles al costo",
        "Cost-sensitive workloads",
        "Cargas sensíveis ao custo",
        "Charges sensibles au coût",
        "Kostensensible Workloads",
    ),
    f!(
        "Carpeta de trabajo de Lucy",
        "Lucy's working folder",
        "Pasta de trabalho da Lucy",
        "Dossier de travail de Lucy",
        "Arbeitsordner von Lucy",
    ),
    f!(
        "Carpeta del skill (o una que contenga varios)",
        "Skill folder (or one holding several)",
        "Pasta da skill (ou uma que contenha várias)",
        "Dossier du skill (ou un dossier qui en contient plusieurs)",
        "Skill-Ordner (oder einer mit mehreren)",
    ),
    f!(
        "Catálogo security/forensics (200+)",
        "Security/forensics catalog (200+)",
        "Catálogo security/forensics (200+)",
        "Catalogue security/forensics (200+)",
        "Katalog security/forensics (200+)",
    ),
    f!("Cerrar", "Close", "Fechar", "Fermer", "Schließen"),
    f!(
        "Cerrar terminal",
        "Close terminal",
        "Fechar terminal",
        "Fermer le terminal",
        "Terminal schließen",
    ),
    f!("Certificados", "Certificates", "Certificados", "Certificats", "Zertifikate"),
    f!("Claro", "Light", "Claro", "Clair", "Hell"),
    f!("Clave privada", "Private key", "Chave privada", "Clé privée", "Privater Schlüssel"),
    f!("Claves API", "API keys", "Chaves API", "Clés API", "API-Schlüssel"),
    f!("Color", "Color", "Cor", "Couleur", "Farbe"),
    f!(
        "Color de acento",
        "Accent colour",
        "Cor de destaque",
        "Couleur d'accent",
        "Akzentfarbe",
    ),
    f!(
        "Comando fallido",
        "Command failed",
        "Comando falhado",
        "Commande échouée",
        "Befehl fehlgeschlagen",
    ),
    f!(
        "Comando lanzado",
        "Command launched",
        "Comando lançado",
        "Commande lancée",
        "Befehl gestartet",
    ),
    f!(
        "Comando o petición para {equipo}…",
        "Command or request for {equipo}…",
        "Comando ou pedido para {equipo}…",
        "Commande ou demande pour {equipo}…",
        "Befehl oder Anfrage für {equipo}…",
    ),
    f!(
        "Comando terminado",
        "Command finished",
        "Comando terminado",
        "Commande terminée",
        "Befehl beendet",
    ),
    f!(
        "Comandos disponibles:",
        "Available commands:",
        "Comandos disponíveis:",
        "Commandes disponibles :",
        "Verfügbare Befehle:",
    ),
    f!(
        "Comparar esta foto con la línea base",
        "Compare this snapshot against the baseline",
        "Comparar esta foto com a linha de base",
        "Comparer cet instantané avec la ligne de base",
        "Diese Aufnahme mit der Baseline vergleichen",
    ),
    f!("Compliance", "Compliance", "Conformidade", "Conformité", "Compliance"),
    f!(
        "Comprobar que responde y con qué sistema",
        "Check it responds and on what system",
        "Verificar se responde e com que sistema",
        "Vérifier qu'elle répond et avec quel système",
        "Erreichbarkeit und System prüfen",
    ),
    f!("Conciso", "Concise", "Conciso", "Concis", "Knapp"),
    f!("Conectado", "Connected", "Ligado", "Connecté", "Verbunden"),
    f!(
        "Conectado en {ms} ms",
        "Connected in {ms} ms",
        "Ligado em {ms} ms",
        "Connecté en {ms} ms",
        "Verbunden in {ms} ms",
    ),
    f!("Conectando…", "Connecting…", "A ligar…", "Connexion…", "Verbinde…"),
    f!("Conectar", "Connect", "Ligar", "Connecter", "Verbinden"),
    f!("Configuración", "Settings", "Configuração", "Réglages", "Einstellungen"),
    f!("Conformes", "Compliant", "Conformes", "Conformes", "Erfüllt"),
    f!("Consolidación", "Consolidation", "Consolidação", "Consolidation", "Konsolidierung"),
    f!(
        "Consolidar ahora",
        "Consolidate now",
        "Consolidar agora",
        "Consolider maintenant",
        "Jetzt konsolidieren",
    ),
    f!(
        "Contenedor (Docker)",
        "Container (Docker)",
        "Contentor (Docker)",
        "Conteneur (Docker)",
        "Container (Docker)",
    ),
    f!("Contraseña", "Password", "Palavra-passe", "Mot de passe", "Passwort"),
    f!("Conversación", "Conversation", "Conversa", "Conversation", "Gespräch"),
    f!("Copia de seguridad", "Backup", "Cópia de segurança", "Sauvegarde", "Sicherung"),
    f!("Copiado", "Copied", "Copiado", "Copié", "Kopiert"),
    f!("Copiar", "Copy", "Copiar", "Copier", "Kopieren"),
    f!(
        "Copiar el informe en CSV",
        "Copy report as CSV",
        "Copiar o relatório em CSV",
        "Copier le rapport en CSV",
        "Bericht als CSV kopieren",
    ),
    f!(
        "Copiar el inventario en CSV",
        "Copy inventory as CSV",
        "Copiar o inventário em CSV",
        "Copier l'inventaire en CSV",
        "Inventar als CSV kopieren",
    ),
    f!("Copiar la ruta", "Copy path", "Copiar o caminho", "Copier le chemin", "Pfad kopieren"),
    f!("Copiar la salida", "Copy output", "Copiar a saída", "Copier la sortie", "Ausgabe kopieren"),
    f!(
        "Copiar las {n} líneas visibles",
        "Copy the {n} visible lines",
        "Copiar as {n} linhas visíveis",
        "Copier les {n} lignes visibles",
        "Die {n} sichtbaren Zeilen kopieren",
    ),
    f!(
        "Copiar toda la salida",
        "Copy all output",
        "Copiar toda a saída",
        "Copier toute la sortie",
        "Ganze Ausgabe kopieren",
    ),
    f!(
        "Correr este comando en este equipo",
        "Run this command on this machine",
        "Correr este comando nesta máquina",
        "Lancer cette commande sur cette machine",
        "Diesen Befehl auf diesem Rechner ausführen",
    ),
    f!("Corriendo…", "Running…", "A correr…", "En cours…", "Läuft…"),
    f!("Cristales", "Crystals", "Cristais", "Cristaux", "Kristalle"),
    f!(
        "Cristales y patrones",
        "Crystals and patterns",
        "Cristais e padrões",
        "Cristaux et motifs",
        "Kristalle und Muster",
    ),
    f!("Crítico", "Critical", "Crítico", "Critique", "Kritisch"),
    f!(
        "Código y Razonamiento",
        "Code and Reasoning",
        "Código e Raciocínio",
        "Code et raisonnement",
        "Code und Reasoning",
    ),
    f!(
        "Cómo está el equipo ahora mismo: procesador, memoria, disco, red, qué servicios \
         automáticos están caídos y qué procesos mandan. Se refresca solo. Con el selector \
         de al lado miras este equipo o cualquiera de los que tengas dados de alta.",
        "How this machine is doing right now: CPU, memory, disk, network, which automatic \
         services are down and which processes are on top. It refreshes on its own. The \
         picker next to it switches between this machine and any other you have set up.",
        "Como está o equipamento neste momento: processador, memória, disco, rede, que \
         serviços automáticos estão em baixo e que processos mandam. Atualiza-se sozinho. \
         Com o seletor ao lado vês este equipamento ou qualquer outro que tenhas registado.",
        "L'état de la machine en ce moment : processeur, mémoire, disque, réseau, quels \
         services automatiques sont tombés et quels processus dominent. Il se rafraîchit \
         tout seul. Le sélecteur à côté bascule entre cette machine et les autres déclarées.",
        "Wie es dem Rechner gerade geht: Prozessor, Speicher, Platte, Netz, welche \
         automatischen Dienste ausgefallen sind und welche Prozesse oben stehen. Aktualisiert \
         sich von selbst. Mit der Auswahl daneben siehst du diesen oder einen anderen Rechner.",
    ),
    f!(
        "DOMINIO/usuario",
        "DOMAIN/user",
        "DOMÍNIO/utilizador",
        "DOMAINE/utilisateur",
        "DOMÄNE/benutzer",
    ),
    f!("Dashboard", "Dashboard", "Painel", "Tableau de bord", "Übersicht"),
    f!(
        "Dashboard de sistema",
        "System dashboard",
        "Painel do sistema",
        "Tableau de bord système",
        "System-Übersicht",
    ),
    f!(
        "Declara que este equipo está como debe. A partir de aquí se puede ver qué cambia.",
        "Declares this machine is as it should be. From here you can see what changes.",
        "Declara que esta máquina está como deve estar. A partir daqui vê-se o que muda.",
        "Déclare que cette machine est dans l'état voulu. À partir de là, on voit ce qui change.",
        "Erklärt, dass dieser Rechner so ist, wie er sein soll. Ab hier lässt sich sehen, was sich ändert.",
    ),
    f!("Del sistema", "System", "Do sistema", "Du système", "Systemvorgabe"),
    f!("Descripción", "Description", "Descrição", "Description", "Beschreibung"),
    f!(
        "Desde el escaneo anterior",
        "Since the previous scan",
        "Desde a análise anterior",
        "Depuis l'analyse précédente",
        "Seit dem letzten Scan",
    ),
    f!(
        "Desinstalar: borra la carpeta del skill",
        "Uninstall: deletes the skill folder",
        "Desinstalar: apaga a pasta do skill",
        "Désinstaller : supprime le dossier du skill",
        "Deinstallieren: löscht den Skill-Ordner",
    ),
    f!(
        "Destilar la sesión en un crystal",
        "Distill the session into a crystal",
        "Destilar a sessão num crystal",
        "Distiller la session dans un crystal",
        "Sitzung zu einem Crystal destillieren",
    ),
    f!("Detallado", "Detailed", "Detalhado", "Détaillé", "Ausführlich"),
    f!("Detener", "Stop", "Parar", "Arrêter", "Stoppen"),
    f!(
        "Detener el dictado",
        "Stop dictation",
        "Parar o ditado",
        "Arrêter la dictée",
        "Diktat stoppen",
    ),
    f!(
        "Detenido, sin error de arranque",
        "Stopped, no startup error",
        "Parado, sem erro de arranque",
        "Arrêté, sans erreur de démarrage",
        "Gestoppt, kein Startfehler",
    ),
    f!(
        "Detenido, sin error de arranque · pulsa para que Lucy lo mire",
        "Stopped, no startup error · click for Lucy to check it",
        "Parado, sem erro de arranque · clica para a Lucy ver",
        "Arrêté, sans erreur de démarrage · clique pour que Lucy regarde",
        "Gestoppt, kein Startfehler · klick, damit Lucy nachschaut",
    ),
    f!(
        "Dictar una regla que Lucy aplica siempre",
        "Set a rule Lucy always applies",
        "Ditar uma regra que a Lucy aplica sempre",
        "Dicter une règle que Lucy applique toujours",
        "Eine Regel festlegen, die Lucy immer anwendet",
    ),
    f!(
        "Dictar — {estado}",
        "Dictate — {estado}",
        "Ditar — {estado}",
        "Dicter — {estado}",
        "Diktieren — {estado}",
    ),
    f!(
        "Dime de dónde: `/skills install C:\\ruta\\al\\skill`. Vale la carpeta de un \
         skill, o una que contenga varios — un repositorio descargado sirve tal cual.",
        "Tell me where: `/skills install C:\\path\\to\\skill`. A skill folder works, or one \
         holding several — a downloaded repository works as it is.",
        "Diz-me de onde: `/skills install C:\\caminho\\para\\skill`. Serve a pasta de um \
         skill, ou uma que contenha vários — um repositório descarregado serve tal como está.",
        "Dis-moi d'où : `/skills install C:\\chemin\\vers\\skill`. Le dossier d'un skill \
         convient, ou un qui en contient plusieurs — un dépôt téléchargé marche tel quel.",
        "Sag mir woher: `/skills install C:\\pfad\\zum\\skill`. Der Ordner eines Skills geht, \
         oder einer mit mehreren — ein heruntergeladenes Repository passt so wie es ist.",
    ),
    f!("Dirección", "Address", "Endereço", "Adresse", "Adresse"),
    f!(
        "Directorio de trabajo",
        "Working directory",
        "Diretório de trabalho",
        "Répertoire de travail",
        "Arbeitsverzeichnis",
    ),
    f!(
        "Directorio de trabajo: {ruta}",
        "Working directory: {ruta}",
        "Diretório de trabalho: {ruta}",
        "Répertoire de travail : {ruta}",
        "Arbeitsverzeichnis: {ruta}",
    ),
    f!("Disco sistema", "System disk", "Disco do sistema", "Disque système", "Systemlaufwerk"),
    f!(
        "Disco {mount} al {pct}%",
        "Disk {mount} at {pct}%",
        "Disco {mount} a {pct}%",
        "Disque {mount} à {pct} %",
        "Laufwerk {mount} bei {pct}%",
    ),
    f!(
        "Disco · aviso",
        "Disk · warning",
        "Disco · aviso",
        "Disque · avertissement",
        "Laufwerk · Warnung",
    ),
    f!(
        "Disco · crítico",
        "Disk · critical",
        "Disco · crítico",
        "Disque · critique",
        "Laufwerk · kritisch",
    ),
    f!("Discos", "Disks", "Discos", "Disques", "Datenträger"),
    f!(
        "Dispositivo de red",
        "Network device",
        "Dispositivo de rede",
        "Équipement réseau",
        "Netzwerkgerät",
    ),
    f!("Documentos", "Documents", "Documentos", "Documents", "Dokumente"),
    f!(
        "Donde Lucy crea ficheros, resuelve nombres sin ruta y ejecuta los comandos que \
         propone.",
        "Where Lucy creates files, resolves names without a path, and runs the commands \
         she proposes.",
        "Onde a Lucy cria ficheiros, resolve nomes sem caminho e executa os comandos que \
         propõe.",
        "Où Lucy crée les fichiers, résout les noms sans chemin et exécute les commandes \
         qu'elle propose.",
        "Wo Lucy Dateien anlegt, Namen ohne Pfad auflöst und die vorgeschlagenen Befehle \
         ausführt.",
    ),
    f!(
        "Dónde guardar la copia",
        "Where to save the copy",
        "Onde guardar a cópia",
        "Où enregistrer la copie",
        "Speicherort der Kopie",
    ),
    f!(
        "Dónde mirar en {equipo}",
        "Where to look on {equipo}",
        "Onde procurar em {equipo}",
        "Où regarder sur {equipo}",
        "Wo auf {equipo} nachsehen",
    ),
    f!("Editar", "Edit", "Editar", "Modifier", "Bearbeiten"),
    f!(
        "Editar equipo",
        "Edit machine",
        "Editar máquina",
        "Modifier la machine",
        "Rechner bearbeiten",
    ),
    f!(
        "Ej. Prod-Web-01",
        "E.g. Prod-Web-01",
        "Ex. Prod-Web-01",
        "Ex. Prod-Web-01",
        "Z. B. Prod-Web-01",
    ),
    f!("Ejecución", "Execution", "Execução", "Exécution", "Ausführung"),
    f!(
        "Ejecutando {n} controles CIS en {equipo}…",
        "Running {n} CIS controls on {equipo}…",
        "A executar {n} controlos CIS em {equipo}…",
        "Exécution de {n} contrôles CIS sur {equipo}…",
        "{n} CIS-Prüfungen laufen auf {equipo}…",
    ),
    f!("Ejecutar", "Run", "Executar", "Exécuter", "Ausführen"),
    f!(
        "Ejecutar consolidación ahora",
        "Run consolidation now",
        "Executar consolidação agora",
        "Lancer la consolidation maintenant",
        "Konsolidierung jetzt ausführen",
    ),
    f!(
        "Ejecutar en {nombre}",
        "Run on {nombre}",
        "Executar em {nombre}",
        "Exécuter sur {nombre}",
        "In {nombre} ausführen",
    ),
    f!(
        "El dictado necesita el modelo de voz {modelo}, que no viene con Lucy: son cientos de megas y el instalador entero pesa veinte. Descárgalo y deja sus tres ficheros en «{ruta}».",
        "Dictation needs the {modelo} voice model, which does not ship with Lucy: it is hundreds of megabytes and the whole installer weighs twenty. Download it and leave its three files in \"{ruta}\".",
        "O ditado precisa do modelo de voz {modelo}, que não vem com a Lucy: são centenas de megas e o instalador inteiro pesa vinte. Descarrega-o e deixa os seus três ficheiros em «{ruta}».",
        "La dictée a besoin du modèle vocal {modelo}, qui n’est pas fourni avec Lucy : il pèse des centaines de mégaoctets et l’installateur entier en fait vingt. Télécharge-le et laisse ses trois fichiers dans « {ruta} ».",
        "Das Diktat braucht das Sprachmodell {modelo}, das nicht mit Lucy kommt: es sind Hunderte Megabyte und das ganze Installationsprogramm wiegt zwanzig. Lade es herunter und leg seine drei Dateien in «{ruta}» ab.",
    ),
    f!(
        "El equipo no informó de ningún disco.",
        "The machine reported no disks.",
        "O equipamento não indicou nenhum disco.",
        "La machine n'a signalé aucun disque.",
        "Der Rechner meldete keine Datenträger.",
    ),
    f!(
        "El escaneo se cortó sin devolver nada.",
        "The scan was cut off without returning anything.",
        "A análise foi interrompida sem devolver nada.",
        "Le scan s'est interrompu sans rien renvoyer.",
        "Der Scan brach ab, ohne etwas zurückzugeben.",
    ),
    f!(
        "El fichero no tiene líneas.",
        "The file has no lines.",
        "O ficheiro não tem linhas.",
        "Le fichier n'a aucune ligne.",
        "Die Datei hat keine Zeilen.",
    ),
    f!(
        "El hilo que traía la respuesta terminó sin decir nada. Es un fallo dentro de \
         Lucy, no del proveedor: vuelve a mandar la orden.",
        "The thread carrying the reply ended without a word. That's a fault inside Lucy, \
         not the provider's: send the order again.",
        "A tarefa que trazia a resposta terminou sem dizer nada. É uma falha dentro da \
         Lucy, não do fornecedor: envie a ordem outra vez.",
        "Le fil qui apportait la réponse s'est terminé sans rien dire. C'est une panne \
         interne de Lucy, pas du fournisseur : renvoyez la commande.",
        "Der Thread mit der Antwort endete wortlos. Das ist ein Fehler in Lucy, nicht \
         beim Anbieter: Schick den Befehl noch einmal.",
    ),
    f!(
        "El modelo de voz está incompleto en {dir}: falta {falta}. Suele ser una copia interrumpida — bórralo y vuelve a ponerlo.",
        "The voice model is incomplete in {dir}: {falta} missing. Usually an interrupted copy — delete it and put it back.",
        "O modelo de voz está incompleto em {dir}: falta {falta}. Costuma ser uma cópia interrompida — apaga-o e volta a pô-lo.",
        "Le modèle vocal est incomplet dans {dir} : il manque {falta}. C’est souvent une copie interrompue — supprime-le et remets-le.",
        "Das Sprachmodell ist unvollständig in {dir}: {falta} fehlt. Meist eine abgebrochene Kopie — lösch es und leg es neu ab.",
    ),
    f!(
        "El modelo se queda corto",
        "The model isn't up to it",
        "O modelo fica aquém",
        "Le modèle ne suffit pas",
        "Das Modell reicht nicht",
    ),
    f!("El más barato", "Cheapest", "O mais barato", "Le moins cher", "Am günstigsten"),
    f!(
        "El paso iba a «{h}», que no está dado de alta. No se ejecuta aquí: sería medir la máquina equivocada.",
        "The step targeted «{h}», which is not registered. It will not run here: that would measure the wrong machine.",
        "O passo ia para «{h}», que não está registado. Não se executa aqui: seria medir a máquina errada.",
        "L'étape visait «{h}», qui n'est pas enregistré. Rien ne s'exécute ici : ce serait mesurer la mauvaise machine.",
        "Der Schritt zielte auf «{h}», und der ist nicht registriert. Wird hier nicht ausgeführt: das würde den falschen Rechner messen.",
    ),
    f!(
        "El razonamiento del agente — pensar · actuar · observar — se registra aquí.",
        "The agent's reasoning — think · act · observe — is logged here.",
        "O raciocínio do agente — pensar · agir · observar — fica registado aqui.",
        "Le raisonnement de l'agent — penser · agir · observer — est enregistré ici.",
        "Das Reasoning des Agenten — denken · handeln · beobachten — wird hier protokolliert.",
    ),
    f!(
        "El servicio «{svc}» es de inicio automático y está parado en este equipo. Dime \
         para qué sirve, si importa que esté parado y qué haría falta para arrancarlo.",
        "The «{svc}» service is set to start automatically and is stopped on this machine. \
         Tell me what it is for, whether it matters that it is stopped and what it would \
         take to start it.",
        "O serviço «{svc}» é de arranque automático e está parado nesta máquina. Diz-me \
         para que serve, se importa que esteja parado e o que seria preciso para o arrancar.",
        "Le service « {svc} » démarre normalement tout seul et il est arrêté sur cette \
         machine. Dis-moi à quoi il sert, si c'est gênant qu'il soit arrêté et ce qu'il \
         faudrait pour le lancer.",
        "Der Dienst «{svc}» startet normalerweise automatisch und ist auf diesem Rechner \
         gestoppt. Sag mir, wofür er da ist, ob das Stoppen ein Problem ist und was nötig \
         wäre, um ihn zu starten.",
    ),
    // ── Dashboard ───────────────────────────────────────────────────────────
    f!(
        "El servicio «{svc}» falló al arrancar en este equipo. Mira por qué, revisa sus \
         últimos eventos y dime si conviene reintentarlo.",
        "The «{svc}» service failed to start on this machine. Find out why, check its \
         latest events and tell me whether it is worth retrying.",
        "O serviço «{svc}» falhou ao arrancar nesta máquina. Vê porquê, revê os seus \
         últimos eventos e diz-me se convém tentar de novo.",
        "Le service « {svc} » n'a pas démarré sur cette machine. Cherche pourquoi, \
         examine ses derniers événements et dis-moi s'il vaut la peine de réessayer.",
        "Der Dienst «{svc}» konnte auf diesem Rechner nicht starten. Finde heraus warum, \
         sieh dir seine letzten Ereignisse an und sag mir, ob ein erneuter Versuch lohnt.",
    ),
    f!(
        "El servidor remoto debe tener WinRM habilitado. Ejecuta allí: Enable-PSRemoting -Force",
        "The remote server needs WinRM enabled. Run this there: Enable-PSRemoting -Force",
        "O servidor remoto tem de ter o WinRM ativado. Executa lá: Enable-PSRemoting -Force",
        "Le serveur distant doit avoir WinRM activé. Exécute là-bas : Enable-PSRemoting -Force",
        "Auf dem Remote-Server muss WinRM aktiviert sein. Führe dort aus: Enable-PSRemoting -Force",
    ),
    f!(
        "El sondeo remoto (`get_remote_health_windows` / `_linux`)",
        "The remote probe (`get_remote_health_windows` / `_linux`)",
        "A sondagem remota (`get_remote_health_windows` / `_linux`)",
        "Le sondage distant (`get_remote_health_windows` / `_linux`)",
        "Die Fernabfrage (`get_remote_health_windows` / `_linux`)",
    ),
    f!(
        "El vigilante avisó de {n}",
        "The watcher raised {n}",
        "O vigilante avisou de {n}",
        "La sentinelle a signalé {n}",
        "Der Wächter hat {n} gemeldet",
    ),
    f!(
        "El vigilante miró y se calló",
        "The watcher looked and stayed quiet",
        "O vigilante olhou e calou-se",
        "La sentinelle a regardé et s'est tue",
        "Der Wächter hat nachgesehen und geschwiegen",
    ),
    f!("Elegir carpeta…", "Choose folder…", "Escolher pasta…", "Choisir un dossier…", "Ordner wählen…"),
    f!(
        "Elige la carpeta de un skill, o una que contenga varios — un repositorio descargado sirve tal cual",
        "Pick a skill's folder, or one holding several — a downloaded repository works as is",
        "Escolhe a pasta de um skill, ou uma que contenha vários — um repositório descarregado serve tal como está",
        "Choisis le dossier d'un skill, ou un dossier qui en contient plusieurs — un dépôt téléchargé convient tel quel",
        "Wähl den Ordner eines Skills, oder einen mit mehreren — ein heruntergeladenes Repository funktioniert direkt",
    ),
    f!("Eliminar", "Delete", "Eliminar", "Supprimer", "Löschen"),
    f!(
        "Embebiendo {h}/{total}…",
        "Embedding {h}/{total}…",
        "A incorporar {h}/{total}…",
        "Vectorisation {h}/{total}…",
        "Einbetten {h}/{total}…",
    ),
    f!(
        "Empezamos",
        "Here we go",
        "Começamos",
        "On commence",
        "Wir fangen an",
    ),
    f!("Enviar", "Send", "Enviar", "Envoyer", "Senden"),
    f!("Equilibrado", "Balanced", "Equilibrado", "Équilibré", "Ausgewogen"),
    f!(
        "Equilibrio inteligencia/costo",
        "Intelligence/cost balance",
        "Equilíbrio inteligência/custo",
        "Équilibre intelligence/coût",
        "Balance Intelligenz/Kosten",
    ),
    f!("Equipo", "Machine", "Máquina", "Machine", "Rechner"),
    f!(
        "Equipo desconocido",
        "Unknown machine",
        "Máquina desconhecida",
        "Poste inconnu",
        "Unbekannter Rechner",
    ),
    f!("Equipos", "Machines", "Máquinas", "Machines", "Rechner"),
    f!(
        "Eran de la orden anterior. Si los sigues queriendo, pídelos otra vez.",
        "They were from the previous instruction. If you still want them, ask again.",
        "Eram da ordem anterior. Se ainda os quiseres, pede-os outra vez.",
        "Elles venaient de l'ordre précédent. Si tu les veux toujours, redemande-les.",
        "Sie gehörten zum vorherigen Befehl. Wenn du sie noch willst, fordere sie erneut an.",
    ),
    f!(
        "Error de conexión",
        "Connection error",
        "Erro de ligação",
        "Erreur de connexion",
        "Verbindungsfehler",
    ),
    f!(
        "Errores recientes",
        "Recent errors",
        "Erros recentes",
        "Erreurs récentes",
        "Aktuelle Fehler",
    ),
    f!(
        "Escanea el software instalado en busca de vulnerabilidades conocidas y dime cómo parcharlas.",
        "Scan installed software for known vulnerabilities and tell me how to patch them.",
        "Analisa o software instalado à procura de vulnerabilidades conhecidas e diz-me como as corrigir.",
        "Analyse les logiciels installés à la recherche de vulnérabilités connues et dis-moi comment les corriger.",
        "Scanne die installierte Software auf bekannte Schwachstellen und sag mir, wie ich sie patche.",
    ),
    f!(
        "Escribe la ruta de un fichero y pulsa Enter.",
        "Type a file path and press Enter.",
        "Escreve o caminho de um ficheiro e prime Enter.",
        "Saisis le chemin d'un fichier et appuie sur Entrée.",
        "Gib einen Dateipfad ein und drück Enter.",
    ),
    f!(
        "Escribe un comando, o dime qué quieres saber y lo traduzco.",
        "Type a command, or tell me what you want to know and I'll translate it.",
        "Escreve um comando, ou diz-me o que queres saber e eu traduzo.",
        "Écris une commande, ou dis-moi ce que tu veux savoir et je la traduis.",
        "Schreib einen Befehl, oder sag mir, was du wissen willst, und ich übersetze es.",
    ),
    f!(
        "Escribe una orden y Lucy la ejecuta — el plan, la salida y el trace \
         se llenan en el workspace →",
        "Type a command and Lucy runs it — the plan, the output and the trace \
         fill up in the workspace →",
        "Escreve uma ordem e a Lucy executa-a — o plano, a saída e o rasto \
         preenchem-se no workspace →",
        "Écris une commande et Lucy l'exécute — le plan, la sortie et la trace \
         se remplissent dans le workspace →",
        "Schreib einen Befehl und Lucy führt ihn aus — Plan, Ausgabe und Spur \
         füllen sich im Workspace →",
    ),
    f!(
        "Escribe una orden…   ·   Shift+Enter = salto de línea",
        "Type a command…   ·   Shift+Enter = new line",
        "Escreve uma ordem…   ·   Shift+Enter = quebra de linha",
        "Écris une commande…   ·   Maj+Entrée = saut de ligne",
        "Schreib einen Befehl…   ·   Shift+Enter = Zeilenumbruch",
    ),
    f!("Escribir", "Write", "Escrever", "Écrire", "Schreiben"),
    f!(
        "Ese equipo ya no está dado de alta.",
        "That machine is no longer registered.",
        "Essa máquina já não está registada.",
        "Ce poste n'est plus enregistré.",
        "Dieser Rechner ist nicht mehr registriert.",
    ),
    f!(
        "Ese equipo ya no está dado de alta. Elige otro en el desplegable.",
        "That machine is no longer registered. Pick another one in the dropdown.",
        "Essa máquina já não está registada. Escolhe outra no menu pendente.",
        "Ce poste n'est plus enregistré. Choisis-en un autre dans la liste déroulante.",
        "Dieser Rechner ist nicht mehr registriert. Wähl einen anderen in der Liste.",
    ),
    f!(
        "Esfuerzo Alto (razonamiento profundo)",
        "High Effort (deep reasoning)",
        "Esforço Alto (raciocínio profundo)",
        "Effort élevé (raisonnement profond)",
        "Hoher Aufwand (tiefes Reasoning)",
    ),
    f!(
        "Esfuerzo Medio (balanceado)",
        "Medium Effort (balanced)",
        "Esforço Médio (equilibrado)",
        "Effort moyen (équilibré)",
        "Mittlerer Aufwand (ausgewogen)",
    ),
    f!(
        "Esta categoría no tiene nada en este equipo.",
        "This category is empty on this machine.",
        "Esta categoria não tem nada nesta máquina.",
        "Cette catégorie ne contient rien sur ce poste.",
        "Diese Kategorie ist auf diesem Rechner leer.",
    ),
    f!(
        "Esta foto pasa a ser la nueva línea base",
        "This snapshot becomes the new baseline",
        "Esta foto passa a ser a nova linha de base",
        "Cet instantané devient la nouvelle ligne de base",
        "Diese Aufnahme wird die neue Baseline",
    ),
    f!("Estado", "Status", "Estado", "État", "Status"),
    f!(
        "Este comando no admite respuestas: WinRM no deja escribirle una vez lanzado. Detenlo y vuelve a lanzarlo sin la parte interactiva.",
        "This command takes no replies: WinRM won't let you write to it once launched. Stop it and run it again without the interactive part.",
        "Este comando não aceita respostas: o WinRM não deixa escrever-lhe depois de lançado. Interrompe-o e volta a lançá-lo sem a parte interativa.",
        "Cette commande n'accepte pas de réponses : WinRM ne permet pas de lui écrire une fois lancée. Arrête-la et relance-la sans la partie interactive.",
        "Dieser Befehl nimmt keine Eingaben an: WinRM lässt nach dem Start kein Schreiben zu. Stoppe ihn und starte ihn ohne den interaktiven Teil neu.",
    ),
    f!("Este equipo", "This machine", "Esta máquina", "Cette machine", "Dieser Rechner"),
    f!(
        "Este equipo no tiene línea base todavía.",
        "This machine has no baseline yet.",
        "Esta máquina ainda não tem linha base.",
        "Ce poste n'a pas encore de ligne de base.",
        "Dieser Rechner hat noch keine Baseline.",
    ),
    f!(
        "Este mismo comando ya falló aquí una vez",
        "This same command already failed here once",
        "Este mesmo comando já falhou aqui uma vez",
        "Cette même commande a déjà échoué ici une fois",
        "Derselbe Befehl ist hier schon einmal fehlgeschlagen",
    ),
    f!(
        "Este mismo comando ya falló aquí {n} veces",
        "This same command already failed here {n} times",
        "Este mesmo comando já falhou aqui {n} vezes",
        "Cette même commande a déjà échoué ici {n} fois",
        "Derselbe Befehl ist hier schon {n} Mal fehlgeschlagen",
    ),
    f!(
        "Este modelo no tiene precio en el catálogo",
        "This model has no price in the catalog",
        "Este modelo não tem preço no catálogo",
        "Ce modèle n'a pas de prix au catalogue",
        "Für dieses Modell gibt es keinen Preis im Katalog",
    ),
    f!(
        "Este paso corre en «{equipo}», no en este equipo. Un comando en otra máquina lo apruebas tú.",
        "This step runs on «{equipo}», not on this machine. A command on another machine is yours to approve.",
        "Este passo corre em «{equipo}», não neste computador. Um comando noutra máquina aprova-lo tu.",
        "Cette étape s'exécute sur « {equipo} », pas sur ce poste. Une commande sur une autre machine, c'est toi qui l'approuves.",
        "Dieser Schritt läuft auf «{equipo}», nicht auf diesem Rechner. Einen Befehl auf einem anderen Rechner gibst du selbst frei.",
    ),
    f!(
        "Este patrón viaja en el prompt de cada turno. Bórralo si está equivocado.",
        "This pattern travels in every turn's prompt. Delete it if it's wrong.",
        "Este padrão viaja no prompt de cada turno. Apaga-o se estiver errado.",
        "Ce motif voyage dans le prompt de chaque tour. Supprime-le s'il est faux.",
        "Dieses Muster reist im Prompt jeder Runde mit. Lösche es, wenn es falsch ist.",
    ),
    f!(
        "Esto es lo que recordaría con «{consulta}»:{como}\n\n{bloque}",
        "This is what I'd recall for «{consulta}»:{como}\n\n{bloque}",
        "Isto é o que recordaria com «{consulta}»:{como}\n\n{bloque}",
        "Voici ce dont je me souviendrais avec «{consulta}» :{como}\n\n{bloque}",
        "Daran würde ich mich bei «{consulta}» erinnern:{como}\n\n{bloque}",
    ),
    f!(
        "Estos servicios fallaron al arrancar en este equipo: {lista}. Mira si tienen una causa común, revisa sus últimos eventos y dime qué haría falta para levantarlos.",
        "These services failed to start on this machine: {lista}. Check whether they share a common cause, review their latest events and tell me what it would take to bring them up.",
        "Estes serviços falharam ao arrancar nesta máquina: {lista}. Vê se têm uma causa comum, revê os últimos eventos e diz-me o que seria preciso para os pôr a funcionar.",
        "Ces services n'ont pas réussi à démarrer sur cette machine : {lista}. Regarde s'ils ont une cause commune, vérifie leurs derniers événements et dis-moi ce qu'il faudrait pour les relancer.",
        "Diese Dienste konnten auf diesem Rechner nicht starten: {lista}. Prüf, ob es eine gemeinsame Ursache gibt, sieh dir ihre letzten Ereignisse an und sag mir, was nötig wäre, um sie zum Laufen zu bringen.",
    ),
    f!("Etiquetas", "Tags", "Etiquetas", "Étiquettes", "Tags"),
    f!(
        "Explorador de memoria (V1)",
        "Memory explorer (V1)",
        "Explorador de memória (V1)",
        "Explorateur de mémoire (V1)",
        "Gedächtnis-Explorer (V1)",
    ),
    f!(
        "Exportar el run (copia al portapapeles)",
        "Export the run (copies to clipboard)",
        "Exportar o run (copia para a área de transferência)",
        "Exporter le run (copie dans le presse-papiers)",
        "Run exportieren (in die Zwischenablage)",
    ),
    f!(
        "Extra Alto (2× el costo de Opus 5)",
        "Extra High (2× Opus 5 cost)",
        "Extra Alto (2× o custo do Opus 5)",
        "Très élevé (2× le coût d'Opus 5)",
        "Extra Hoch (2× Kosten von Opus 5)",
    ),
    f!(
        "Extra Alto (coding/agéntico)",
        "Extra High (coding/agentic)",
        "Extra Alto (código/agêntico)",
        "Très élevé (code/agentique)",
        "Extra Hoch (Coding/agentisch)",
    ),
    f!(
        "Extra Alto (generación anterior)",
        "Extra High (previous generation)",
        "Extra Alto (geração anterior)",
        "Très élevé (génération précédente)",
        "Extra Hoch (Vorgängergeneration)",
    ),
    f!(
        "Extra Alto (tareas más duras)",
        "Extra High (hardest tasks)",
        "Extra Alto (tarefas mais duras)",
        "Très élevé (tâches les plus dures)",
        "Extra Hoch (härteste Aufgaben)",
    ),
    f!(
        "Extrayendo el texto del PDF…",
        "Extracting the PDF text…",
        "A extrair o texto do PDF…",
        "Extraction du texte du PDF…",
        "PDF-Text wird extrahiert…",
    ),
    f!(
        "Extrayendo texto…",
        "Extracting text…",
        "A extrair texto…",
        "Extraction du texte…",
        "Text wird extrahiert…",
    ),
    f!("FALLAS", "FAILURES", "FALHAS", "ÉCHECS", "FEHLER"),
    f!("Falla", "Failure", "Falha", "Échec", "Fehler"),
    f!("Fallas", "Failures", "Falhas", "Échecs", "Fehler"),
    f!(
        "Falta: {campos}",
        "Missing: {campos}",
        "Em falta: {campos}",
        "Manque : {campos}",
        "Fehlt: {campos}",
    ),
    f!("Fichero escrito", "File written", "Ficheiro escrito", "Fichier écrit", "Datei geschrieben"),
    f!(
        "Fijada · entra en todos los prompts",
        "Pinned · goes into every prompt",
        "Fixada · entra em todos os prompts",
        "Épinglée · entre dans tous les prompts",
        "Angeheftet · geht in jeden Prompt",
    ),
    f!(
        "Fijada: entra en TODOS los prompts. Pulsa para soltarla.",
        "Pinned: goes into EVERY prompt. Click to unpin.",
        "Fixada: entra em TODOS os prompts. Clica para a soltar.",
        "Épinglée : entre dans TOUS les prompts. Clique pour la détacher.",
        "Angeheftet: geht in ALLE Prompts. Klick zum Lösen.",
    ),
    f!(
        "Fijar línea base",
        "Set baseline",
        "Fixar linha de base",
        "Définir la ligne de base",
        "Baseline festlegen",
    ),
    f!(
        "Fijar: que Lucy la tenga presente siempre, venga o no al caso",
        "Pin: Lucy keeps it in mind always, relevant or not",
        "Fixar: que a Lucy a tenha sempre presente, venha ou não a propósito",
        "Épingler : que Lucy l'ait toujours en tête, utile ou pas",
        "Anheften: Lucy hat sie immer präsent, ob passend oder nicht",
    ),
    f!(
        "Filtrar por esta etiqueta",
        "Filter by this tag",
        "Filtrar por esta etiqueta",
        "Filtrer par cette étiquette",
        "Nach diesem Tag filtern",
    ),
    f!(
        "Formato: fork_task:nombre-corto|qué tiene que averiguar. Hacen falta las dos partes.",
        "Format: fork_task:short-name|what it has to find out. Both parts are required.",
        "Formato: fork_task:nome-curto|o que tem de descobrir. São precisas as duas partes.",
        "Format : fork_task:nom-court|ce qu'il doit découvrir. Les deux parties sont nécessaires.",
        "Format: fork_task:kurzname|was herausgefunden werden soll. Beide Teile sind nötig.",
    ),
    f!(
        "Foto incompleta: {p}",
        "Incomplete snapshot: {p}",
        "Foto incompleta: {p}",
        "Instantané incomplet : {p}",
        "Snapshot unvollständig: {p}",
    ),
    f!(
        "Frontera (trabajo profesional complejo)",
        "Frontier (complex professional work)",
        "Fronteira (trabalho profissional complexo)",
        "Frontière (travail professionnel complexe)",
        "Frontier (komplexe Profi-Arbeit)",
    ),
    f!(
        "Fundidas {memorias} memorias en {grupos} grupos.",
        "Merged {memorias} memories into {grupos} groups.",
        "Fundidas {memorias} memórias em {grupos} grupos.",
        "Fusion de {memorias} mémoires en {grupos} groupes.",
        "{memorias} Erinnerungen in {grupos} Gruppen zusammengeführt.",
    ),
    f!("Fundir", "Merge", "Fundir", "Fusionner", "Zusammenführen"),
    f!(
        "Gastado de verdad",
        "Actually spent",
        "Gasto real",
        "Dépensé réellement",
        "Tatsächlich ausgegeben",
    ),    f!(
        "Google vía NVIDIA",
        "Google via NVIDIA",
        "Google via NVIDIA",
        "Google via NVIDIA",
        "Google über NVIDIA",
    ),

    f!(
        "Grafo de conocimiento (V1)",
        "Knowledge graph (V1)",
        "Grafo de conhecimento (V1)",
        "Graphe de connaissances (V1)",
        "Wissensgraph (V1)",
    ),
    f!("Guardado", "Saved", "Guardado", "Enregistré", "Gespeichert"),
    f!("Guardar", "Save", "Guardar", "Enregistrer", "Speichern"),
    f!(
        "Guardar copia…",
        "Save a copy…",
        "Guardar cópia…",
        "Enregistrer une copie…",
        "Kopie speichern…",
    ),
    f!(
        "Hay {activos} activos y en el prompt entran {caben}: los que sobren no se aplican. Apaga los que ya no manden.",
        "There are {activos} active and {caben} fit in the prompt: the extras don't apply. Turn off the ones that no longer call the shots.",
        "Há {activos} ativos e no prompt cabem {caben}: os que sobrarem não se aplicam. Desliga os que já não mandam.",
        "Il y a {activos} actifs et {caben} entrent dans le prompt : les autres ne s'appliquent pas. Désactive ceux qui ne commandent plus.",
        "Aktiv sind {activos}, in den Prompt passen {caben}: der Rest wird nicht angewendet. Schalte ab, was nicht mehr gilt.",
    ),
    f!(
        "He ejecutado el comando que propusiste y esta es su salida literal. {cola}\n\n$ {cmd}\n\n{body}",
        "I ran the command you proposed and this is its literal output. {cola}\n\n$ {cmd}\n\n{body}",
        "Executei o comando que propuseste e esta é a saída literal. {cola}\n\n$ {cmd}\n\n{body}",
        "J'ai exécuté la commande que tu as proposée, voici sa sortie littérale. {cola}\n\n$ {cmd}\n\n{body}",
        "Ich habe den vorgeschlagenen Befehl ausgeführt, das ist seine wörtliche Ausgabe. {cola}\n\n$ {cmd}\n\n{body}",
    ),
    f!("Historial", "History", "Histórico", "Historique", "Verlauf"),
    f!("Idioma", "Language", "Idioma", "Langue", "Sprache"),
    f!(
        "Importancia alta · se recuerda antes que las demás",
        "High importance · recalled before the rest",
        "Importância alta · é recordada antes das outras",
        "Importance haute · rappelée avant les autres",
        "Hohe Wichtigkeit · wird vor den anderen erinnert",
    ),
    f!(
        "Importancia baja · la última en entrar si no cabe todo",
        "Low importance · the last one in when not everything fits",
        "Importância baixa · a última a entrar se não couber tudo",
        "Importance basse · la dernière à entrer si tout ne tient pas",
        "Geringe Wichtigkeit · kommt zuletzt rein, wenn nicht alles passt",
    ),
    f!(
        "Importancia normal",
        "Normal importance",
        "Importância normal",
        "Importance normale",
        "Normale Wichtigkeit",
    ),
    f!("Ingiriendo…", "Ingesting…", "A ingerir…", "Ingestion…", "Einlesen…"),
    f!(
        "Instalados: {lista}.",
        "Installed: {lista}.",
        "Instalados: {lista}.",
        "Installés : {lista}.",
        "Installiert: {lista}.",
    ),
    f!("Instalar…", "Install…", "Instalar…", "Installer…", "Installieren…"),
    f!("Interfaz", "Interface", "Interface", "Interface", "Oberfläche"),
    f!("Inventario", "Inventory", "Inventário", "Inventaire", "Bestand"),
    f!(
        "La búsqueda semántica necesita Ollama con un modelo de embeddings (ollama pull nomic-embed-text).",
        "Semantic search needs Ollama with an embeddings model (ollama pull nomic-embed-text).",
        "A pesquisa semântica precisa do Ollama com um modelo de embeddings (ollama pull nomic-embed-text).",
        "La recherche sémantique a besoin d'Ollama avec un modèle d'embeddings (ollama pull nomic-embed-text).",
        "Die semantische Suche braucht Ollama mit einem Embedding-Modell (ollama pull nomic-embed-text).",
    ),
    f!(
        "La exploración se cortó sin devolver nada.",
        "The browse was cut off without returning anything.",
        "A exploração foi interrompida sem devolver nada.",
        "L'exploration s'est interrompue sans rien renvoyer.",
        "Das Durchsuchen brach ab, ohne etwas zurückzugeben.",
    ),
    f!(
        "La lectura remota se cortó sin devolver nada.",
        "The remote read was cut off without returning anything.",
        "A leitura remota foi interrompida sem devolver nada.",
        "La lecture distante s'est interrompue sans rien renvoyer.",
        "Das Remote-Lesen brach ab, ohne etwas zurückzugeben.",
    ),
    f!(
        "La memoria en disco",
        "Memory on disk",
        "A memória em disco",
        "La mémoire sur disque",
        "Das Gedächtnis auf der Festplatte",
    ),
    f!(
        "La nube más barata",
        "Cheapest cloud",
        "A nuvem mais barata",
        "Le cloud le moins cher",
        "Die günstigste Cloud",
    ),
    f!(
        "La revisión se cortó sin devolver nada.",
        "The review was cut off without returning anything.",
        "A revisão foi interrompida sem devolver nada.",
        "La vérification s'est interrompue sans rien renvoyer.",
        "Die Prüfung brach ab, ohne etwas zurückzugeben.",
    ),
    f!(
        "La ronda por los equipos avisó de {n}",
        "The round of machines raised {n}",
        "A ronda pelos equipamentos avisou de {n}",
        "La tournée des machines a signalé {n}",
        "Die Runde über die Rechner hat {n} gemeldet",
    ),
    f!(
        "La ronda por los equipos no dio nada",
        "The round of machines found nothing",
        "A ronda pelos equipamentos não deu nada",
        "La tournée des machines n'a rien donné",
        "Die Runde über die Rechner ergab nichts",
    ),
    f!(
        "La salida de cada comando aparece aquí en vivo mientras el agente trabaja.",
        "Each command's output appears here live while the agent works.",
        "A saída de cada comando aparece aqui em direto enquanto o agente trabalha.",
        "La sortie de chaque commande s'affiche ici en direct pendant que l'agent travaille.",
        "Die Ausgabe jedes Befehls erscheint hier live, während der Agent arbeitet.",
    ),
    f!(
        "La sonda terminó sin contestar.",
        "The probe ended without answering.",
        "A sonda terminou sem responder.",
        "La sonde s'est terminée sans répondre.",
        "Die Abfrage endete ohne Antwort.",
    ),
    f!(
        "La tarea se cortó sin devolver nada.",
        "The task was cut off without returning anything.",
        "A tarefa foi interrompida sem devolver nada.",
        "La tâche s'est interrompue sans rien renvoyer.",
        "Die Aufgabe brach ab, ohne etwas zurückzugeben.",
    ),
    f!(
        "Lanzada «{id}». Sigue con lo tuyo y recógela con wait_task:{id} cuando la necesites.",
        "Launched «{id}». Carry on with your work and pick it up with wait_task:{id} when you need it.",
        "Lançada «{id}». Continua com o teu trabalho e recolhe-a com wait_task:{id} quando precisares.",
        "«{id}» lancée. Continue ce que tu fais et récupère-la avec wait_task:{id} quand tu en as besoin.",
        "Gestartet: «{id}». Mach weiter und hol sie mit wait_task:{id} ab, wenn du sie brauchst.",
    ),
    f!(
        "Leer la cola de este fichero",
        "Tail this file",
        "Ler o fim deste ficheiro",
        "Lire la fin de ce fichier",
        "Das Ende dieser Datei lesen",
    ),
    f!(
        "Leer la cola del fichero",
        "Tail the file",
        "Ler o fim do ficheiro",
        "Lire la fin du fichier",
        "Das Ende der Datei lesen",
    ),
    f!("Legado", "Legacy", "Legado", "Hérité", "Legacy"),
    f!("Limpiar", "Clear", "Limpar", "Effacer", "Leeren"),
    f!(
        "Limpiar el chat actual",
        "Clear the current chat",
        "Limpar o chat atual",
        "Effacer la conversation en cours",
        "Aktuellen Chat leeren",
    ),
    f!(
        "Limpiar el workspace",
        "Clear the workspace",
        "Limpar o workspace",
        "Nettoyer le workspace",
        "Workspace leeren",
    ),
    f!(
        "Limpiar la pantalla",
        "Clear screen",
        "Limpar o ecrã",
        "Effacer l'écran",
        "Bildschirm leeren",
    ),
    f!(
        "Lista de runbooks (V1)",
        "Runbook list (V1)",
        "Lista de runbooks (V1)",
        "Liste des runbooks (V1)",
        "Runbook-Liste (V1)",
    ),
    f!("Listo para operar", "Ready to operate", "Pronto a operar", "Opérationnel", "Einsatzbereit"),
    f!(
        "Listo para operar en {equipo}",
        "Ready to operate on {equipo}",
        "Pronto a operar em {equipo}",
        "Prêt à intervenir sur {equipo}",
        "Einsatzbereit auf {equipo}",
    ),
    f!(
        "Llama más Reciente",
        "Latest Llama",
        "Llama mais Recente",
        "Llama le plus récent",
        "Neuestes Llama",
    ),
    f!(
        "Lleva {n} pasadas sin sacar nada, desde {cuando}",
        "It has gone {n} passes without producing anything, since {cuando}",
        "Leva {n} passagens sem produzir nada, desde {cuando}",
        "Cela fait {n} passages sans rien produire, depuis {cuando}",
        "Läuft seit {n} Durchgängen ohne Ergebnis, seit {cuando}",
    ),
    f!(
        "Llevas {gastado} en esta sesión y el tope está en {tope}. El automático se apaga; súbelo en Configuración o sigue paso a paso.",
        "You've spent {gastado} this session and the cap is {tope}. Auto mode turns off; raise it in Settings or go step by step.",
        "Já gastaste {gastado} nesta sessão e o limite está em {tope}. O automático desliga-se; aumenta-o em Configuração ou segue passo a passo.",
        "Tu as dépensé {gastado} dans cette session et le plafond est à {tope}. Le mode automatique se désactive ; augmente-le dans Configuration ou continue pas à pas.",
        "Du hast in dieser Sitzung {gastado} verbraucht, das Limit liegt bei {tope}. Der Automatikmodus geht aus; erhöhe es unter Einstellungen oder geh Schritt für Schritt vor.",
    ),
    f!(
        "Lo ingerido alimenta el recuerdo y a pdf_search. Los secretos se redactan al entrar.",
        "What goes in feeds recall and pdf_search. Secrets are redacted on the way in.",
        "O que é ingerido alimenta a memória e o pdf_search. Os segredos são ocultados à entrada.",
        "Ce qui est ingéré alimente la mémoire et pdf_search. Les secrets sont masqués à l'entrée.",
        "Das Aufgenommene speist die Erinnerung und pdf_search. Geheimnisse werden beim Einlesen geschwärzt.",
    ),
    f!(
        "Lo que Lucy recuerda: hechos sueltos, sesiones destiladas, manuales que le has \
         dado y los principios que le has puesto. Casi todo se escribe solo. Entra aquí \
         cuando repita algo viejo o no encuentre lo que ya le contaste. Buscar por \
         significado necesita Ollama.",
        "What Lucy remembers: loose facts, distilled sessions, manuals you gave her and the \
         principles you set. Almost all of it writes itself. Come here when she repeats \
         something stale or cannot find what you already told her. Searching by meaning \
         needs Ollama.",
        "O que a Lucy recorda: factos soltos, sessões destiladas, manuais que lhe deste e \
         os princípios que lhe puseste. Quase tudo se escreve sozinho. Entra aqui quando \
         repetir algo velho ou não encontrar o que já lhe contaste. Procurar por significado \
         precisa do Ollama.",
        "Ce dont Lucy se souvient : faits isolés, sessions distillées, manuels que vous lui \
         avez donnés et principes que vous avez fixés. Presque tout s'écrit tout seul. Venez \
         ici quand elle répète du vieux ou ne retrouve pas ce que vous lui aviez dit. La \
         recherche par le sens exige Ollama.",
        "Woran Lucy sich erinnert: einzelne Fakten, destillierte Sitzungen, Handbücher, die \
         du ihr gegeben hast, und die Grundsätze, die du gesetzt hast. Fast alles schreibt \
         sich von selbst. Komm hierher, wenn sie Altes wiederholt oder nicht findet, was du \
         ihr schon gesagt hast. Die Suche nach Bedeutung braucht Ollama.",
    ),
    f!(
        "Lo que se da de alta una vez: la clave del proveedor, tu nombre, el modelo y el \
         aspecto. Sin ninguna clave guardada solo funcionan los modelos locales de Ollama. \
         Aquí están también el tope de gasto de la sesión y la copia de seguridad de la \
         memoria.",
        "The things you set up once: the provider key, your name, the model and the looks. \
         With no key saved, only local Ollama models work. The session spend cap and the \
         memory backup live here too.",
        "O que se define uma vez: a chave do fornecedor, o teu nome, o modelo e o aspeto. \
         Sem nenhuma chave guardada só funcionam os modelos locais do Ollama. Aqui estão \
         também o limite de gasto da sessão e a cópia de segurança da memória.",
        "Ce qui se règle une seule fois : la clé du fournisseur, votre nom, le modèle et \
         l'apparence. Sans clé enregistrée, seuls les modèles locaux d'Ollama fonctionnent. \
         Le plafond de dépense et la sauvegarde de la mémoire sont ici aussi.",
        "Was man einmal einrichtet: den Anbieterschlüssel, deinen Namen, das Modell und das \
         Aussehen. Ohne gespeicherten Schlüssel laufen nur lokale Ollama-Modelle. Auch das \
         Ausgabenlimit der Sitzung und die Sicherung des Gedächtnisses stehen hier.",
    ),
    f!("Log", "Log", "Log", "Journal", "Log"),
    f!(
        "Los archivos que Lucy edita o escribe aparecen aquí con su diff.",
        "Files Lucy edits or writes appear here with their diff.",
        "Os ficheiros que a Lucy edita ou escreve aparecem aqui com o respetivo diff.",
        "Les fichiers que Lucy modifie ou écrit apparaissent ici avec leur diff.",
        "Dateien, die Lucy bearbeitet oder schreibt, erscheinen hier mit Diff.",
    ),
    f!(
        "Los dos trabajos corren solos por vencimiento — también si el programa estuvo cerrado cuando tocaba. Esto es para no esperar al plazo.",
        "Both jobs run on their own when due — even if the program was closed at the time. This is for when you don't want to wait.",
        "Os dois trabalhos correm sozinhos por vencimento — mesmo que o programa estivesse fechado na altura. Isto é para não esperar pelo prazo.",
        "Les deux tâches se lancent seules à échéance — même si le programme était fermé le moment venu. Ceci sert à ne pas attendre le délai.",
        "Beide Jobs laufen bei Fälligkeit von selbst — auch wenn das Programm zum Termin geschlossen war. Das hier ist, um nicht auf die Frist zu warten.",
    ),
    f!(
        "Lucy desglosa la tarea en pasos y los va marcando conforme avanza.",
        "Lucy breaks the task into steps and checks them off as work progresses.",
        "A Lucy divide a tarefa em passos e vai marcando-os à medida que avança.",
        "Lucy découpe la tâche en étapes et les coche au fur et à mesure.",
        "Lucy zerlegt die Aufgabe in Schritte und hakt sie beim Vorankommen ab.",
    ),
    f!(
        "Lucy los pide sola cuando encajan. Para forzar uno, díselo por su nombre.",
        "Lucy asks for them herself when they fit. To force one, name it.",
        "A Lucy pede-os sozinha quando encaixam. Para forçar um, diz-lhe o nome.",
        "Lucy les demande d'elle-même quand ils conviennent. Pour en forcer un, cite son nom.",
        "Lucy fordert sie selbst an, wenn sie passen. Um einen zu erzwingen, nenne ihn beim Namen.",
    ),
    f!(
        "Lucy todavía no ha apuntado nada sobre ti. Lo hace sola cuando le cuentas algo que le servirá otro día.",
        "Lucy hasn't noted anything about you yet. She does it on her own when you tell her something that will help another day.",
        "A Lucy ainda não apontou nada sobre ti. Fá-lo sozinha quando lhe contas algo que lhe servirá noutro dia.",
        "Lucy n'a encore rien noté sur toi. Elle le fait seule quand tu lui dis quelque chose qui lui servira un autre jour.",
        "Lucy hat noch nichts über dich notiert. Sie macht das von selbst, wenn du ihr etwas erzählst, das ihr an einem anderen Tag nützt.",
    ),
    f!(
        "Lucy ya corre como administrador: esto no es un problema de privilegios.",
        "Lucy already runs as administrator: this is not a privileges problem.",
        "A Lucy já corre como administrador: isto não é um problema de privilégios.",
        "Lucy tourne déjà en administrateur : ce n'est pas un problème de privilèges.",
        "Lucy läuft bereits als Administrator: das ist kein Rechteproblem.",
    ),
    f!(
        "Línea base: {etiqueta} · {cuando}",
        "Baseline: {etiqueta} · {cuando}",
        "Linha base: {etiqueta} · {cuando}",
        "Ligne de base : {etiqueta} · {cuando}",
        "Baseline: {etiqueta} · {cuando}",
    ),
    f!("Mantenimiento", "Maintenance", "Manutenção", "Maintenance", "Wartung"),
    f!("Marcar leídos", "Mark as read", "Marcar como lidos", "Marquer comme lus", "Als gelesen markieren"),
    f!(
        "Max (problemas frontera)",
        "Max (frontier problems)",
        "Máx (problemas de fronteira)",
        "Max (problèmes frontière)",
        "Max (Frontier-Probleme)",
    ),
    f!("Maximizar", "Maximize", "Maximizar", "Agrandir", "Maximieren"),
    f!(
        "Medio (ahorro de costo)",
        "Medium (cost saving)",
        "Médio (poupança de custo)",
        "Moyen (économie de coût)",
        "Mittel (Kostenersparnis)",
    ),
    f!(
        "Medio (generación anterior)",
        "Medium (previous generation)",
        "Médio (geração anterior)",
        "Moyen (génération précédente)",
        "Mittel (Vorgängergeneration)",
    ),
    f!(
        "Medio (sensible al costo)",
        "Medium (cost-sensitive)",
        "Médio (sensível ao custo)",
        "Moyen (sensible au coût)",
        "Mittel (kostensensibel)",
    ),
    f!("Memoria", "Memory", "Memória", "Mémoire", "Gedächtnis"),
    f!("Memorias", "Memories", "Memórias", "Mémoires", "Erinnerungen"),
    f!(
        "Memorias automáticas",
        "Automatic memories",
        "Memórias automáticas",
        "Mémoires automatiques",
        "Automatische Erinnerungen",
    ),
    f!("Minimizar", "Minimize", "Minimizar", "Réduire", "Minimieren"),
    f!("Modelo activo", "Active model", "Modelo ativo", "Modèle actif", "Aktives Modell"),
    f!(
        "Modelo de voz listo",
        "Voice model ready",
        "Modelo de voz pronto",
        "Modèle vocal prêt",
        "Sprachmodell bereit",
    ),
    f!(
        "Modelo y comportamiento",
        "Model and behaviour",
        "Modelo e comportamento",
        "Modèle et comportement",
        "Modell und Verhalten",
    ),
    f!(
        "Modo **{n}** puesto — {d}\n\nA partir de ahora enmarco todo en él. Se quita con `/preset clear`.",
        "Mode **{n}** set — {d}\n\nFrom now on I frame everything in it. Clear it with `/preset clear`.",
        "Modo **{n}** definido — {d}\n\nA partir de agora enquadro tudo nele. Remove-se com `/preset clear`.",
        "Mode **{n}** activé — {d}\n\nDésormais, je cadre tout dans ce mode. Se retire avec `/preset clear`.",
        "Modus **{n}** gesetzt — {d}\n\nAb jetzt ordne ich alles darin ein. Entfernen mit `/preset clear`.",
    ),
    f!(
        "Modo **{p}** quitado. Vuelvo a contestar libremente.",
        "Mode **{p}** removed. I'm answering freely again.",
        "Modo **{p}** removido. Volto a responder livremente.",
        "Mode **{p}** retiré. Je réponds de nouveau librement.",
        "Modus **{p}** entfernt. Ich antworte wieder frei.",
    ),
    f!(
        "Modo activo: **{p}**.\n\nQuítalo con `/preset clear`.",
        "Active mode: **{p}**.\n\nClear it with `/preset clear`.",
        "Modo ativo: **{p}**.\n\nRemove-o com `/preset clear`.",
        "Mode actif : **{p}**.\n\nRetire-le avec `/preset clear`.",
        "Aktiver Modus: **{p}**.\n\nEntferne ihn mit `/preset clear`.",
    ),
    f!("Modo privacidad", "Privacy mode", "Modo privacidade", "Mode confidentialité", "Privatmodus"),
    f!(
        "Modo privacidad (sólo LLM local)",
        "Privacy mode (local LLM only)",
        "Modo privacidade (só LLM local)",
        "Mode confidentialité (LLM local uniquement)",
        "Datenschutzmodus (nur lokales LLM)",
    ),
    f!(
        "Modo privacidad **activado**. Nada sale de este equipo.\n\n⚠ {e}",
        "Privacy mode **on**. Nothing leaves this machine.\n\n⚠ {e}",
        "Modo privacidade **ativado**. Nada sai deste computador.\n\n⚠ {e}",
        "Mode confidentialité **activé**. Rien ne sort de ce poste.\n\n⚠ {e}",
        "Datenschutzmodus **an**. Nichts verlässt diesen Rechner.\n\n⚠ {e}",
    ),
    f!(
        "Modo privacidad **activado**. Nada sale de este equipo. El modelo actual (`{modelo}`) es local, así que puedes seguir.",
        "Privacy mode **on**. Nothing leaves this machine. The current model (`{modelo}`) is local, so you can carry on.",
        "Modo privacidade **ativado**. Nada sai deste computador. O modelo atual (`{modelo}`) é local, por isso podes continuar.",
        "Mode confidentialité **activé**. Rien ne sort de ce poste. Le modèle actuel (`{modelo}`) est local, tu peux donc continuer.",
        "Datenschutzmodus **an**. Nichts verlässt diesen Rechner. Das aktuelle Modell (`{modelo}`) ist lokal, du kannst also weitermachen.",
    ),
    f!(
        "Modo privacidad **apagado**. Vuelven a estar disponibles los modelos de nube.",
        "Privacy mode **off**. Cloud models are available again.",
        "Modo privacidade **desligado**. Voltam a estar disponíveis os modelos na nuvem.",
        "Mode confidentialité **désactivé**. Les modèles cloud sont de nouveau disponibles.",
        "Datenschutzmodus **aus**. Cloud-Modelle sind wieder verfügbar.",
    ),
    f!(
        "Modo privacidad: nada sale de este equipo. Solo modelos locales de Ollama. Se apaga con /privacy.",
        "Privacy mode: nothing leaves this machine. Local Ollama models only. Turn it off with /privacy.",
        "Modo privacidade: nada sai desta máquina. Só modelos locais do Ollama. Desliga-se com /privacy.",
        "Mode confidentialité : rien ne sort de cette machine. Uniquement des modèles locaux d'Ollama. Se désactive avec /privacy.",
        "Datenschutzmodus: Nichts verlässt diesen Rechner. Nur lokale Ollama-Modelle. Aus mit /privacy.",
    ),
    f!(
        "Máxima Inteligencia",
        "Maximum Intelligence",
        "Inteligência Máxima",
        "Intelligence maximale",
        "Maximale Intelligenz",
    ),
    f!("NVIDIA Flagship", "NVIDIA Flagship", "NVIDIA de Topo", "NVIDIA Flagship", "NVIDIA Flagship"),
    f!("NVIDIA Máximo", "NVIDIA Max", "NVIDIA Máximo", "NVIDIA Maximum", "NVIDIA Maximum"),
    f!(
        "Nada ejecutado aún",
        "Nothing run yet",
        "Ainda nada executado",
        "Encore rien d'exécuté",
        "Noch nichts ausgeführt",
    ),
    f!(
        "Nada ha cambiado desde la línea base.",
        "Nothing has changed since the baseline.",
        "Nada mudou desde a linha de base.",
        "Rien n’a changé depuis la ligne de base.",
        "Seit der Baseline hat sich nichts geändert.",
    ),
    f!(
        "Nada ha cambiado desde la línea base.\n({n} puertos dinámicos ignorados — el sistema los reparte en cada arranque.)",
        "Nothing has changed since the baseline.\n({n} dynamic ports ignored — the system reassigns them on every boot.)",
        "Nada mudou desde a linha base.\n({n} portas dinâmicas ignoradas — o sistema distribui-as em cada arranque.)",
        "Rien n'a changé depuis la ligne de base.\n({n} ports dynamiques ignorés — le système les réattribue à chaque démarrage.)",
        "Seit der Baseline hat sich nichts geändert.\n({n} dynamische Ports ignoriert — das System vergibt sie bei jedem Start neu.)",
    ),
    f!(
        "Nada que copiar todavía",
        "Nothing to copy yet",
        "Nada para copiar ainda",
        "Rien à copier pour l'instant",
        "Noch nichts zum Kopieren",
    ),
    f!(
        "Ninguna repetida entre las {n} más recientes.",
        "None repeated among the {n} most recent.",
        "Nenhuma repetida entre as {n} mais recentes.",
        "Aucune répétition parmi les {n} plus récentes.",
        "Keine Wiederholung unter den {n} neuesten.",
    ),
    f!(
        "Ninguno en este estado.",
        "None in this state.",
        "Nenhum neste estado.",
        "Aucun dans cet état.",
        "Keiner in diesem Zustand.",
    ),
    f!(
        "Ninguno. Un skill es una carpeta con un SKILL.md dentro; Lucy los ve y pide el que encaje.",
        "None. A skill is a folder with a SKILL.md inside; Lucy sees them and asks for the one that fits.",
        "Nenhum. Um skill é uma pasta com um SKILL.md dentro; a Lucy vê-os e pede o que encaixa.",
        "Aucun. Un skill est un dossier qui contient un SKILL.md ; Lucy les voit et demande celui qui convient.",
        "Keiner. Ein Skill ist ein Ordner mit einer SKILL.md darin; Lucy sieht sie und fordert den passenden an.",
    ),
    f!(
        "Ningún documento todavía. Un manual ingerido contesta preguntas sin que nadie lo mencione — es la fuente principal de la memoria.",
        "No documents yet. An ingested manual answers questions without anyone mentioning it — it's the main source of memory.",
        "Ainda nenhum documento. Um manual ingerido responde a perguntas sem que ninguém o mencione — é a fonte principal da memória.",
        "Aucun document pour l'instant. Un manuel ingéré répond aux questions sans que personne ne le mentionne — c'est la source principale de la mémoire.",
        "Noch kein Dokument. Ein eingelesenes Handbuch beantwortet Fragen, ohne dass es jemand erwähnt — es ist die Hauptquelle der Erinnerung.",
    ),
    f!(
        "Ningún modelo coincide",
        "No models match",
        "Nenhum modelo coincide",
        "Aucun modèle ne correspond",
        "Kein Modell passt",
    ),
    f!(
        "No había ningún trozo sin vector.",
        "There were no chunks without a vector.",
        "Não havia nenhum fragmento sem vetor.",
        "Aucun fragment n'était sans vecteur.",
        "Es gab kein Fragment ohne Vektor.",
    ),
    f!(
        "No hay ficheros de log en {dir}.",
        "No log files in {dir}.",
        "Não há ficheiros de log em {dir}.",
        "Aucun fichier de log dans {dir}.",
        "Keine Log-Dateien in {dir}.",
    ),
    f!(
        "No hay nada visible que copiar",
        "Nothing visible to copy",
        "Não há nada visível para copiar",
        "Rien de visible à copier",
        "Nichts Sichtbares zum Kopieren",
    ),
    f!(
        "No hay ningún modo puesto, y tampoco hay skills instalados.",
        "No mode is set, and there are no skills installed.",
        "Não há nenhum modo definido, nem skills instalados.",
        "Aucun mode actif, et aucun skill installé.",
        "Kein Modus gesetzt, und keine Skills installiert.",
    ),
    f!(
        "No hay ningún modo puesto.\n\nFija uno con `/preset <nombre>`: {hay}",
        "No mode is set.\n\nSet one with `/preset <name>`: {hay}",
        "Não há nenhum modo definido.\n\nDefine um com `/preset <nome>`: {hay}",
        "Aucun mode actif.\n\nDéfinis-en un avec `/preset <nom>` : {hay}",
        "Es ist kein Modus gesetzt.\n\nSetze einen mit `/preset <name>`: {hay}",
    ),
    f!(
        "No hay ningún skill llamado «{a}». Los que hay: {hay}.",
        "There's no skill called «{a}». The ones there are: {hay}.",
        "Não há nenhum skill chamado «{a}». Os que há: {hay}.",
        "Il n'y a aucun skill nommé «{a}». Ceux qui existent : {hay}.",
        "Es gibt keinen Skill namens «{a}». Vorhanden: {hay}.",
    ),
    f!(
        "No pude apuntar eso",
        "I couldn't note that down",
        "Não consegui anotar isso",
        "Je n'ai pas pu noter ça",
        "Das konnte ich nicht notieren",
    ),
    f!(
        "No pude capturar tu pantalla: {e}",
        "I couldn't capture your screen: {e}",
        "Não consegui capturar o teu ecrã: {e}",
        "Je n'ai pas pu capturer ton écran : {e}",
        "Ich konnte deinen Bildschirm nicht erfassen: {e}",
    ),
    f!(
        "No queda margen para escribir «{ruta}». Apruébalo tú.",
        "No budget left to write «{ruta}». Approve it yourself.",
        "Não resta margem para escrever «{ruta}». Aprova-o tu.",
        "Il ne reste plus de marge pour écrire «{ruta}». À toi de l'approuver.",
        "Kein Spielraum mehr, um «{ruta}» zu schreiben. Genehmige es selbst.",
    ),
    f!(
        "No se enviará: {motivo}",
        "Won't be sent: {motivo}",
        "Não será enviado: {motivo}",
        "Ne sera pas envoyé : {motivo}",
        "Wird nicht gesendet: {motivo}",
    ),
    f!(
        "No se pudieron leer: {e}",
        "Couldn't be read: {e}",
        "Não foi possível ler: {e}",
        "Lecture impossible : {e}",
        "Konnten nicht gelesen werden: {e}",
    ),
    f!("No se pudo", "Not measured", "Não foi possível", "Non mesuré", "Nicht messbar"),
    f!(
        "No se pudo consultar esta categoría — el motivo está arriba.",
        "Couldn't query this category — the reason is above.",
        "Não foi possível consultar esta categoria — o motivo está acima.",
        "Impossible d'interroger cette catégorie — la raison est au-dessus.",
        "Diese Kategorie konnte nicht abgefragt werden — der Grund steht oben.",
    ),
    f!(
        "No se pudo consultar — el motivo está arriba.",
        "Couldn't query — the reason is above.",
        "Não foi possível consultar — o motivo está acima.",
        "Impossible de consulter — la raison est ci-dessus.",
        "Abfrage nicht möglich — der Grund steht oben.",
    ),
    f!(
        "No se pudo enviar la respuesta: {e}",
        "Couldn't send the reply: {e}",
        "Não foi possível enviar a resposta: {e}",
        "Impossible d'envoyer la réponse : {e}",
        "Antwort konnte nicht gesendet werden: {e}",
    ),
    f!(
        "No se pudo escribir",
        "Could not write",
        "Não foi possível escrever",
        "Écriture impossible",
        "Schreiben fehlgeschlagen",
    ),
    f!(
        "No se pudo grabar: {e}",
        "Could not record: {e}",
        "Não foi possível gravar: {e}",
        "Impossible d'enregistrer : {e}",
        "Aufnahme nicht möglich: {e}",
    ),
    f!(
        "No se pudo guardar",
        "Could not save",
        "Não foi possível guardar",
        "Échec de l'enregistrement",
        "Speichern fehlgeschlagen",
    ),
    f!(
        "No se pudo leer «{ruta}» en {equipo}: {e}",
        "Could not read «{ruta}» on {equipo}: {e}",
        "Não foi possível ler «{ruta}» em {equipo}: {e}",
        "Lecture de «{ruta}» impossible sur {equipo} : {e}",
        "Lesen von «{ruta}» auf {equipo} fehlgeschlagen: {e}",
    ),
    f!(
        "No se pudo leer «{ruta}»: {e}",
        "Could not read «{ruta}»: {e}",
        "Não foi possível ler «{ruta}»: {e}",
        "Impossible de lire « {ruta} » : {e}",
        "«{ruta}» konnte nicht gelesen werden: {e}",
    ),
    f!(
        "No se pudo listar «{ruta}»: {e}",
        "Could not list «{ruta}»: {e}",
        "Não foi possível listar «{ruta}»: {e}",
        "Impossible de lister «{ruta}» : {e}",
        "Auflisten von «{ruta}» fehlgeschlagen: {e}",
    ),
    f!(
        "No se pudo registrar en la auditoría",
        "Could not write to the audit log",
        "Não foi possível registar na auditoria",
        "Enregistrement dans l'audit impossible",
        "Eintrag im Audit-Protokoll fehlgeschlagen",
    ),
    f!(
        "No se pudo resolver tu perfil de usuario.",
        "Couldn't resolve your user profile.",
        "Não foi possível resolver o teu perfil de utilizador.",
        "Impossible de résoudre ton profil utilisateur.",
        "Dein Benutzerprofil ließ sich nicht ermitteln.",
    ),
    f!(
        "No se pudo revisar: {e}",
        "Could not check: {e}",
        "Não foi possível verificar: {e}",
        "Impossible de vérifier : {e}",
        "Prüfung fehlgeschlagen: {e}",
    ),
    f!(
        "No se pudo sondear el equipo",
        "Could not probe the machine",
        "Não se conseguiu sondar o equipamento",
        "Impossible de sonder la machine",
        "Der Rechner konnte nicht abgefragt werden",
    ),
    f!(
        "No se pudo traducir: {e}",
        "Could not translate: {e}",
        "Não foi possível traduzir: {e}",
        "Impossible de traduire : {e}",
        "Übersetzung nicht möglich: {e}",
    ),
    f!(
        "No se pudo transcribir: {e}",
        "Could not transcribe: {e}",
        "Não foi possível transcrever: {e}",
        "Transcription impossible : {e}",
        "Transkription fehlgeschlagen: {e}",
    ),
    f!(
        "No supe convertir eso en un comando.",
        "I couldn't turn that into a command.",
        "Não soube converter isso num comando.",
        "Je n'ai pas su transformer ça en commande.",
        "Ich konnte daraus keinen Befehl machen.",
    ),
    f!("Nombre", "Name", "Nome", "Nom", "Name"),
    f!("Nueva Terminal", "New Terminal", "Novo Terminal", "Nouveau terminal", "Neues Terminal"),
    f!("Nueva terminal", "New terminal", "Novo terminal", "Nouveau terminal", "Neues Terminal"),
    f!(
        "Nuevo equipo remoto",
        "New remote machine",
        "Nova máquina remota",
        "Nouvelle machine distante",
        "Neuer Remote-Rechner",
    ),
    f!("Nunca", "Never", "Nunca", "Jamais", "Nie"),
    f!(
        "Nunca ha corrido en esta base — correrá en la próxima comprobación.",
        "Never run on this database — it will run at the next check.",
        "Nunca correu nesta base — vai correr na próxima verificação.",
        "N'a jamais tourné sur cette base — tournera à la prochaine vérification.",
        "Ist in dieser Datenbank noch nie gelaufen — läuft bei der nächsten Prüfung.",
    ),
    f!("Núcleos", "Cores", "Núcleos", "Cœurs", "Kerne"),
    f!(
        "Ocultar detalle",
        "Hide details",
        "Ocultar detalhe",
        "Masquer le détail",
        "Details ausblenden",
    ),
    f!("Ollama offline", "Ollama offline", "Ollama offline", "Ollama hors ligne", "Ollama offline"),
    f!(
        "Ollama · modelos locales",
        "Ollama · local models",
        "Ollama · modelos locais",
        "Ollama · modèles locaux",
        "Ollama · lokale Modelle",
    ),
    f!(
        "Ollama · {n} modelos",
        "Ollama · {n} models",
        "Ollama · {n} modelos",
        "Ollama · {n} modèles",
        "Ollama · {n} Modelle",
    ),
    f!("Operador", "Operator", "Operador", "Opérateur", "Operator"),
    f!("Orden enviada", "Instruction sent", "Ordem enviada", "Ordre envoyé", "Befehl gesendet"),
    f!(
        "Ordenar por esta columna",
        "Sort by this column",
        "Ordenar por esta coluna",
        "Trier par cette colonne",
        "Nach dieser Spalte sortieren",
    ),
    f!("Oscuro", "Dark", "Escuro", "Sombre", "Dunkel"),
    f!("PROCESO", "PROCESS", "PROCESSO", "PROCESSUS", "PROZESS"),
    f!(
        "Pasa los controles CIS al equipo y te dice cuáles no cumple y con qué se ha \
         mirado cada uno. Hay que pulsar Escanear. Señala lo que está flojo; arreglarlo \
         sigue siendo cosa tuya.",
        "Runs the CIS checks against this machine and tells you which ones it fails, and \
         what each was checked against. You have to press Scan. It points at what is weak; \
         fixing it is still your call.",
        "Passa os controlos CIS ao equipamento e diz-te quais não cumpre e com que foi \
         verificado cada um. Tens de carregar em Analisar. Aponta o que está fraco; \
         corrigir continua a ser contigo.",
        "Applique les contrôles CIS à ce poste et indique lesquels échouent, et sur quoi \
         chacun a été vérifié. Il faut appuyer sur Analyser. Il signale ce qui cloche ; \
         le corriger reste votre décision.",
        "Prüft diesen Rechner gegen die CIS-Vorgaben und zeigt, welche er nicht erfüllt \
         und woran das jeweils gemessen wurde. Du musst auf Scannen drücken. Es zeigt die \
         Schwachstellen; das Beheben bleibt deine Sache.",
    ),
    f!(
        "Pasos que Lucy ha encadenado sola en esta orden. Al llegar al tope se apaga y sigue aprobando el operador.",
        "Steps Lucy has chained on her own in this command. At the cap she stops and the operator approves again.",
        "Passos que a Lucy encadeou sozinha nesta ordem. Ao chegar ao limite desliga-se e o operador volta a aprovar.",
        "Étapes que Lucy a enchaînées seule pour cette demande. Arrivée au plafond, elle s'arrête et l'opérateur reprend les approbations.",
        "Schritte, die Lucy in diesem Befehl allein verkettet hat. Am Limit schaltet sie ab, und der Operator gibt weiter frei.",
    ),
    f!("Patrones", "Patterns", "Padrões", "Motifs", "Muster"),
    f!(
        "Pausar la actualización",
        "Pause refresh",
        "Pausar a atualização",
        "Suspendre l'actualisation",
        "Aktualisierung pausieren",
    ),
    f!("Pensando…", "Thinking…", "A pensar…", "Réflexion…", "Denkt nach…"),
    f!(
        "Personalidad de Lucy",
        "Lucy's personality",
        "Personalidade da Lucy",
        "Personnalité de Lucy",
        "Lucys Persönlichkeit",
    ),
    f!(
        "Picker de skills ejecutables",
        "Runnable skills picker",
        "Seletor de skills executáveis",
        "Sélecteur de skills exécutables",
        "Auswahl ausführbarer Skills",
    ),
    f!(
        "Pide el catálogo de modelos — no gasta",
        "Fetches the model catalog — costs nothing",
        "Pede o catálogo de modelos — não gasta",
        "Demande le catalogue de modèles — ne coûte rien",
        "Fragt den Modellkatalog ab — kostet nichts",
    ),
    f!("Plan ▸", "Plan ▸", "Plano ▸", "Plan ▸", "Plan ▸"),
    f!("Plan ▸ {n}", "Plan ▸ {n}", "Plano ▸ {n}", "Plan ▸ {n}", "Plan ▸ {n}"),
    f!(
        "Plegar el carril — vuelve con el botón de la cabecera",
        "Collapse the rail — bring it back from the header button",
        "Recolher o carril — volta com o botão do cabeçalho",
        "Replier le rail — il revient avec le bouton de l'en-tête",
        "Leiste einklappen — kommt über die Kopfzeile zurück",
    ),
    f!(
        "Plegar las alertas",
        "Collapse alerts",
        "Recolher os alertas",
        "Replier les alertes",
        "Warnungen einklappen",
    ),
    f!(
        "Por CLAVE, no por contraseña: la confianza se establece antes. Autoriza tu clave pública en el servidor (`~/.ssh/authorized_keys`) o ten la privada cargada en `ssh-agent`.",
        "By KEY, not password: trust is established beforehand. Authorize your public key on the server (`~/.ssh/authorized_keys`) or have the private one loaded in `ssh-agent`.",
        "Por CHAVE, não por palavra-passe: a confiança estabelece-se antes. Autoriza a tua chave pública no servidor (`~/.ssh/authorized_keys`) ou tem a privada carregada no `ssh-agent`.",
        "Par CLÉ, pas par mot de passe : la confiance s'établit avant. Autorise ta clé publique sur le serveur (`~/.ssh/authorized_keys`) ou garde la privée chargée dans `ssh-agent`.",
        "Mit SCHLÜSSEL, nicht mit Passwort: Das Vertrauen wird vorher hergestellt. Hinterlege deinen öffentlichen Schlüssel auf dem Server (`~/.ssh/authorized_keys`) oder halte den privaten im `ssh-agent` geladen.",
    ),
    f!(
        "Potencia Equilibrada",
        "Balanced Power",
        "Potência Equilibrada",
        "Puissance équilibrée",
        "Ausgewogene Leistung",
    ),
    f!(
        "PowerShell · PTY",
        "PowerShell · PTY",
        "PowerShell · PTY",
        "PowerShell · PTY",
        "PowerShell · PTY",
    ),
    f!(
        "Presets de framing (AD, Hyper-V, SQL…)",
        "Framing presets (AD, Hyper-V, SQL…)",
        "Presets de framing (AD, Hyper-V, SQL…)",
        "Presets de framing (AD, Hyper-V, SQL…)",
        "Framing-Presets (AD, Hyper-V, SQL…)",
    ),
    f!("Principios", "Principles", "Princípios", "Principes", "Prinzipien"),
    f!("Privilegios", "Privileges", "Privilégios", "Privilèges", "Rechte"),
    f!("Probando…", "Testing…", "A testar…", "Test…", "Test läuft…"),
    f!("Probar", "Test", "Testar", "Tester", "Testen"),
    f!(
        "Probar conexión",
        "Test connection",
        "Testar ligação",
        "Tester la connexion",
        "Verbindung testen",
    ),
    f!(
        "Procesos que más ocupan",
        "Top processes",
        "Processos que mais ocupam",
        "Processus les plus lourds",
        "Größte Prozesse",
    ),
    f!("Protocolo", "Protocol", "Protocolo", "Protocole", "Protokoll"),
    f!(
        "Proyección de polaridad de un texto",
        "Polarity projection for a text",
        "Projeção de polaridade de um texto",
        "Projection de polarité d'un texte",
        "Polaritätsprojektion eines Texts",
    ),
    f!("Puerto", "Port", "Porta", "Port", "Port"),
    f!("Puertos", "Ports", "Portas", "Ports", "Ports"),
    f!(
        "Pulsa Escanear para hacerle una foto a este equipo.",
        "Press Scan to take a snapshot of this machine.",
        "Carrega em Analisar para tirar uma fotografia a esta máquina.",
        "Appuie sur Scanner pour prendre une photo de ce poste.",
        "Drück auf Scannen, um eine Momentaufnahme dieses Rechners zu machen.",
    ),
    f!(
        "Pulsa Escanear para pasar los controles CIS a este equipo.",
        "Press Scan to run the CIS controls on this machine.",
        "Carrega em Analisar para passar os controlos CIS a esta máquina.",
        "Appuie sur Analyser pour passer les contrôles CIS sur cette machine.",
        "Drück auf Scannen, um die CIS-Prüfungen auf diesem Rechner laufen zu lassen.",
    ),
    f!(
        "Pulsa Sondear para pedirle su estado a este equipo.",
        "Press Probe to ask this machine for its status.",
        "Carregue em Sondar para pedir o estado a este equipamento.",
        "Appuyez sur Sonder pour demander son état à cette machine.",
        "Auf Abfragen drücken, um den Zustand dieses Rechners zu erfragen.",
    ),
    f!(
        "Pídele las cosas en español y Lucy propone el comando, lo ejecuta si lo apruebas y \
         te cuenta qué salió. Cada pestaña es una conversación aparte, con su propio plan y \
         su propia traza. Todo lo que ejecuta queda anotado en el Log Viewer.",
        "Ask for things in plain words and Lucy proposes the command, runs it if you approve \
         and tells you how it went. Each tab is a separate conversation with its own plan and \
         its own trace. Everything she runs is recorded in the Log Viewer.",
        "Pede as coisas por palavras e a Lucy propõe o comando, executa-o se o aprovares e \
         conta-te como correu. Cada separador é uma conversa à parte, com o seu plano e o seu \
         rasto. Tudo o que executa fica anotado no Log Viewer.",
        "Demandez les choses en clair : Lucy propose la commande, l'exécute si vous \
         l'approuvez et vous dit ce que ça a donné. Chaque onglet est une conversation à \
         part, avec son plan et sa trace. Tout ce qu'elle exécute est consigné dans le Log \
         Viewer.",
        "Sag es in eigenen Worten: Lucy schlägt den Befehl vor, führt ihn nach deiner \
         Freigabe aus und berichtet, was herauskam. Jeder Tab ist ein eigenes Gespräch mit \
         eigenem Plan und eigener Spur. Alles Ausgeführte steht im Log Viewer.",
    ),
    f!(
        "Que Lucy lo olvide",
        "Make Lucy forget it",
        "Que a Lucy se esqueça",
        "Que Lucy l'oublie",
        "Lucy soll es vergessen",
    ),
    f!(
        "Que un modelo local reescriba los avisos",
        "Let a local model rewrite the alerts",
        "Que um modelo local reescreva os avisos",
        "Qu'un modèle local réécrive les alertes",
        "Ein lokales Modell die Hinweise umschreiben lassen",
    ),
    f!("Quitar", "Remove", "Remover", "Retirer", "Entfernen"),
    f!("Quitar en lote", "Bulk remove", "Remover em lote", "Retirer en lot", "Mehrere entfernen"),
    f!("Qué falta", "What's missing", "O que falta", "Ce qui manque", "Was fehlt"),
    f!(
        "Qué se ha ejecutado, con qué resultado y cuánto tardó — la auditoría de Lucy, en \
         vivo. En «Archivo» miras en cambio los ficheros de log de una carpeta del equipo, \
         que es otra cosa.",
        "What has been run, with what result and how long it took — Lucy's audit trail, \
         live. The «File» tab instead shows the log files in a folder on this machine, \
         which is a different thing.",
        "O que foi executado, com que resultado e quanto demorou — a auditoria da Lucy, ao \
         vivo. Em «Ficheiro» vês antes os ficheiros de log de uma pasta do equipamento, que \
         é outra coisa.",
        "Ce qui a été exécuté, avec quel résultat et en combien de temps — le journal \
         d'audit de Lucy, en direct. L'onglet « Fichier » montre plutôt les fichiers de log \
         d'un dossier de la machine, ce qui est autre chose.",
        "Was ausgeführt wurde, mit welchem Ergebnis und wie lange es dauerte — Lucys \
         Prüfprotokoll, live. Unter «Datei» siehst du stattdessen die Logdateien eines \
         Ordners auf dem Rechner, was etwas anderes ist.",
    ),
    f!("RAM al {pct}%", "RAM at {pct}%", "RAM a {pct}%", "RAM à {pct} %", "RAM bei {pct}%"),
    f!("RAM alta ({pct}%)", "High RAM ({pct}%)", "RAM alta ({pct}%)", "RAM élevée ({pct}%)", "Hohe RAM-Auslastung ({pct}%)"),
    f!("RAM · aviso", "RAM · warning", "RAM · aviso", "RAM · avertissement", "RAM · Warnung"),
    f!("RAM · crítico", "RAM · critical", "RAM · crítico", "RAM · critique", "RAM · kritisch"),
    f!("Razonamiento", "Reasoning", "Raciocínio", "Raisonnement", "Denkprozess"),
    f!(
        "Razonamiento insignia",
        "Flagship reasoning",
        "Raciocínio de topo",
        "Raisonnement phare",
        "Flaggschiff-Reasoning",
    ),
    f!(
        "Razonamiento más fuerte",
        "Strongest reasoning",
        "Raciocínio mais forte",
        "Raisonnement le plus fort",
        "Stärkstes Reasoning",
    ),
    f!(
        "Reanudar la actualización",
        "Resume refresh",
        "Retomar a atualização",
        "Reprendre l'actualisation",
        "Aktualisierung fortsetzen",
    ),
    f!("Rechazar", "Reject", "Rejeitar", "Refuser", "Ablehnen"),
    f!(
        "Recuerdo por significado",
        "Recall by meaning",
        "Recordação por significado",
        "Souvenir par le sens",
        "Erinnern nach Bedeutung",
    ),
    f!(
        "Recuperar memorias por consulta",
        "Retrieve memories by query",
        "Recuperar memórias por consulta",
        "Récupérer des mémoires par requête",
        "Erinnerungen per Abfrage abrufen",
    ),
    f!("Red", "Network", "Rede", "Réseau", "Netzwerk"),
    f!(
        "Referencia completa de comandos",
        "Full command reference",
        "Referência completa de comandos",
        "Référence complète des commandes",
        "Vollständige Befehlsreferenz",
    ),
    f!(
        "Reflexionar ahora",
        "Reflect now",
        "Refletir agora",
        "Réfléchir maintenant",
        "Jetzt reflektieren",
    ),
    f!("Reflexión", "Reflection", "Reflexão", "Réflexion", "Reflexion"),
    f!(
        "Reglas que aplico siempre:\n\n{lista}\n\nPara añadir una: `/principio en producción avisa antes de reiniciar un servicio`.",
        "Rules I always apply:\n\n{lista}\n\nTo add one: `/principio in production warn before restarting a service`.",
        "Regras que aplico sempre:\n\n{lista}\n\nPara adicionar uma: `/principio em produção avisa antes de reiniciar um serviço`.",
        "Règles que j'applique toujours :\n\n{lista}\n\nPour en ajouter une : `/principio en production préviens avant de redémarrer un service`.",
        "Regeln, die ich immer befolge:\n\n{lista}\n\nZum Hinzufügen: `/principio in der Produktion vor dem Neustart eines Dienstes warnen`.",
    ),
    f!("Rehacer", "Redo", "Refazer", "Rétablir", "Wiederholen"),
    f!(
        "Rehaciendo los vectores que faltaban…",
        "Rebuilding the missing vectors…",
        "A refazer os vetores em falta…",
        "Recalcul des vecteurs manquants…",
        "Fehlende Vektoren werden neu erstellt…",
    ),
    f!("Rehaciendo…", "Rebuilding…", "A refazer…", "Reconstruction…", "Neuaufbau…"),
    f!("Reintentar", "Retry", "Tentar de novo", "Réessayer", "Wiederholen"),
    f!(
        "Relaciones tipadas entre memorias",
        "Typed relations between memories",
        "Relações tipadas entre memórias",
        "Relations typées entre mémoires",
        "Typisierte Beziehungen zwischen Erinnerungen",
    ),
    f!(
        "Rendimiento de frontera sostenido",
        "Sustained frontier performance",
        "Desempenho de fronteira sustentado",
        "Performance de frontière soutenue",
        "Dauerhafte Frontier-Leistung",
    ),
    f!(
        "Respuesta para el comando en curso (p. ej. y) …",
        "Reply for the running command (e.g. y) …",
        "Resposta para o comando em curso (p. ex. y) …",
        "Réponse pour la commande en cours (p. ex. y) …",
        "Antwort für den laufenden Befehl (z. B. y) …",
    ),
    f!("Restaurar", "Restore", "Restaurar", "Restaurer", "Wiederherstellen"),
    f!(
        "Resume lo que dice. Si hace falta otro comando para responder a lo que se te pidió, propónlo; si ya tienes la respuesta, dala y no propongas nada más.",
        "Summarize what it says. If another command is needed to answer what you were asked, propose it; if you already have the answer, give it and propose nothing more.",
        "Resume o que diz. Se for preciso outro comando para responder ao que te pediram, propõe-o; se já tens a resposta, dá-a e não proponhas mais nada.",
        "Résume ce que ça dit. S'il faut une autre commande pour répondre à ce qu'on t'a demandé, propose-la ; si tu as déjà la réponse, donne-la et ne propose rien de plus.",
        "Fass zusammen, was da steht. Wenn für die gestellte Frage noch ein Befehl nötig ist, schlag ihn vor; wenn du die Antwort schon hast, gib sie und schlag nichts weiter vor.",
    ),
    f!(
        "Resume los errores más recientes del registro de eventos del sistema (últimas 24 h).",
        "Summarize the most recent errors in the system event log (last 24 h).",
        "Resume os erros mais recentes do registo de eventos do sistema (últimas 24 h).",
        "Résume les erreurs les plus récentes du journal d'événements système (dernières 24 h).",
        "Fasse die neuesten Fehler aus dem Systemereignisprotokoll zusammen (letzte 24 h).",
    ),
    f!(
        "Revisa la salud del sistema (CPU, RAM, disco, servicios) y dame un resumen del estado.",
        "Check system health (CPU, RAM, disk, services) and give me a summary of the state.",
        "Verifica a saúde do sistema (CPU, RAM, disco, serviços) e dá-me um resumo do estado.",
        "Vérifie la santé du système (CPU, RAM, disque, services) et donne-moi un résumé de l'état.",
        "Prüfe den Systemzustand (CPU, RAM, Festplatte, Dienste) und gib mir eine Zusammenfassung des Zustands.",
    ),
    f!(
        "Revisión detenida.",
        "Review stopped.",
        "Revisão parada.",
        "Vérification arrêtée.",
        "Prüfung gestoppt.",
    ),
    f!(
        "Rápido y Eficiente",
        "Fast and Efficient",
        "Rápido e Eficiente",
        "Rapide et efficace",
        "Schnell und effizient",
    ),
    f!(
        "Rápido y Ligero",
        "Fast and Light",
        "Rápido e Leve",
        "Rapide et léger",
        "Schnell und leicht",
    ),
    f!(
        "Salida retenida por el guardrail",
        "Output held by the guardrail",
        "Saída retida pelo guardrail",
        "Sortie retenue par le guardrail",
        "Ausgabe vom guardrail zurückgehalten",
    ),
    f!(
        "Salió con código de error",
        "Exited with an error code",
        "Saiu com código de erro",
        "Terminé avec un code d'erreur",
        "Mit Fehlercode beendet",
    ),
    f!(
        "Salió con código de error · pulsa para que Lucy lo investigue",
        "Exited with an error code · click for Lucy to look into it",
        "Saiu com código de erro · clica para a Lucy investigar",
        "Terminé avec un code d'erreur · clique pour que Lucy enquête",
        "Mit Fehlercode beendet · klick, damit Lucy nachsieht",
    ),
    f!(
        "Salud del sistema",
        "System health",
        "Saúde do sistema",
        "Santé du système",
        "Systemzustand",
    ),
    f!("Saludable", "Healthy", "Saudável", "Sain", "Gesund"),
    f!(
        "Se acabó el margen del automático ({gastado} de {max}). El siguiente paso lo apruebas tú — mirar cuesta 1 punto y cambiar algo cuesta {cambio}.",
        "The auto budget is spent ({gastado} of {max}). You approve the next step — looking costs 1 point, changing something costs {cambio}.",
        "Acabou a margem do automático ({gastado} de {max}). O próximo passo aprova-lo tu — olhar custa 1 ponto e mudar algo custa {cambio}.",
        "La marge du mode automatique est épuisée ({gastado} sur {max}). L'étape suivante, c'est toi qui l'approuves — regarder coûte 1 point, changer quelque chose en coûte {cambio}.",
        "Das Budget des Automatikmodus ist aufgebraucht ({gastado} von {max}). Den nächsten Schritt gibst du frei — Nachsehen kostet 1 Punkt, etwas ändern {cambio}.",
    ),
    f!(
        "Se guardan en el Credential Manager de Windows, en el mismo sitio del que las lee la app de escritorio. Ollama no necesita clave: es local.",
        "They're saved in the Windows Credential Manager, the same place the desktop app reads them from. Ollama needs no key: it's local.",
        "Guardam-se no Credential Manager do Windows, no mesmo sítio de onde a app de desktop as lê. O Ollama não precisa de chave: é local.",
        "Elles sont enregistrées dans le Credential Manager de Windows, là où l'appli de bureau les lit. Ollama n'a pas besoin de clé : il est local.",
        "Sie werden im Credential Manager von Windows gespeichert, dort, wo die Desktop-App sie liest. Ollama braucht keinen Schlüssel: läuft lokal.",
    ),
    f!("Se hace tarde", "Getting late", "Vai ficando tarde", "Il se fait tard", "Es wird spät"),
    f!(
        "Se migra el bloque entero o no se migra.",
        "The whole block migrates or none of it does.",
        "Migra-se o bloco inteiro ou não se migra.",
        "Le bloc se migre en entier, ou pas du tout.",
        "Der Block wird ganz migriert oder gar nicht.",
    ),
    f!(
        "Seguimos",
        "Still at it",
        "Continuamos",
        "On continue",
        "Weiter geht’s",
    ),
    f!("Servicios", "Services", "Serviços", "Services", "Dienste"),
    f!(
        "Servicios detenidos",
        "Stopped services",
        "Serviços parados",
        "Services arrêtés",
        "Gestoppte Dienste",
    ),
    f!(
        "Servidor / Shell",
        "Server / Shell",
        "Servidor / Shell",
        "Serveur / Shell",
        "Server / Shell",
    ),
    f!(
        "Sin actividad registrada.",
        "No activity recorded.",
        "Sem atividade registada.",
        "Aucune activité enregistrée.",
        "Keine Aktivität erfasst.",
    ),
    f!("Sin artefactos", "No artifacts", "Sem artefactos", "Aucun artefact", "Keine Artefakte"),
    f!(
        "Sin coincidencias.",
        "No matches.",
        "Sem correspondências.",
        "Aucune correspondance.",
        "Keine Treffer.",
    ),
    f!("Sin conexión", "Disconnected", "Sem ligação", "Déconnecté", "Nicht verbunden"),
    f!(
        "Sin datos todavía",
        "No data yet",
        "Ainda sem dados",
        "Aucune donnée pour l'instant",
        "Noch keine Daten",
    ),
    f!(
        "Sin equipos remotos dados de alta",
        "No remote machines registered",
        "Sem máquinas remotas registadas",
        "Aucune machine distante enregistrée",
        "Keine Remote-Rechner eingetragen",
    ),
    f!(
        "Sin equipos remotos dados de alta todavía.",
        "No remote machines registered yet.",
        "Ainda não há equipamentos remotos registados.",
        "Aucune machine distante enregistrée pour l'instant.",
        "Noch keine Remote-Rechner eingetragen.",
    ),
    f!(
        "Sin equipos remotos.",
        "No remote machines.",
        "Sem máquinas remotas.",
        "Aucune machine distante.",
        "Keine Remote-Rechner.",
    ),
    f!(
        "Sin línea base para este equipo.",
        "No baseline for this machine.",
        "Sem linha de base para esta máquina.",
        "Aucune ligne de base pour cette machine.",
        "Keine Baseline für diesen Rechner.",
    ),
    f!("Sin medir", "Unmeasured", "Sem medir", "Non mesurés", "Ungemessen"),
    f!("Sin plan todavía", "No plan yet", "Ainda sem plano", "Pas encore de plan", "Noch kein Plan"),
    f!(
        "Sin privilegios y con UAC desactivado: hay que abrir Lucy con una cuenta de \
         administrador.",
        "No privileges and UAC is off: you have to open Lucy with an administrator account.",
        "Sem privilégios e com o UAC desativado: tens de abrir a Lucy com uma conta de \
         administrador.",
        "Sans privilèges et avec l'UAC désactivé : il faut ouvrir Lucy avec un compte \
         administrateur.",
        "Keine Rechte und UAC ist aus: Du musst Lucy mit einem Administratorkonto öffnen.",
    ),
    f!(
        "Sin privilegios · UAC desactivado",
        "No privileges · UAC disabled",
        "Sem privilégios · UAC desativado",
        "Sans privilèges · UAC désactivé",
        "Ohne Adminrechte · UAC deaktiviert",
    ),
    f!(
        "Sin privilegios · UAC disponible",
        "No privileges · UAC available",
        "Sem privilégios · UAC disponível",
        "Sans privilèges · UAC disponible",
        "Ohne Adminrechte · UAC verfügbar",
    ),
    f!("Sin resultados", "No results", "Sem resultados", "Aucun résultat", "Keine Treffer"),
    f!("Sistema", "System", "Sistema", "Système", "System"),
    f!(
        "Skill propuesto: {nombre}",
        "Proposed skill: {nombre}",
        "Skill proposta: {nombre}",
        "Skill proposé : {nombre}",
        "Skill vorgeschlagen: {nombre}",
    ),
    f!("Skills", "Skills", "Skills", "Skills", "Skills"),
    f!("Software", "Software", "Software", "Logiciels", "Software"),
    f!(
        "Soltar para adjuntar",
        "Drop to attach",
        "Largar para anexar",
        "Déposer pour joindre",
        "Zum Anhängen loslassen",
    ),
    f!(
        "Soltar para adjuntar {encima} ficheros",
        "Drop to attach {encima} files",
        "Largar para anexar {encima} ficheiros",
        "Déposer pour joindre {encima} fichiers",
        "{encima} Dateien zum Anhängen loslassen",
    ),
    f!("Sondeando…", "Probing…", "A sondar…", "Sondage…", "Wird abgefragt…"),
    f!("Sondear", "Probe", "Sondar", "Sonder", "Abfragen"),
    f!("Sub-agentes", "Sub-agents", "Subagentes", "Sous-agents", "Sub-Agenten"),
    f!(
        "Síntesis forense de incidente",
        "Forensic incident synthesis",
        "Síntese forense de incidente",
        "Synthèse forensique d'incident",
        "Forensische Vorfallssynthese",
    ),
    f!("Tareas", "Tasks", "Tarefas", "Tâches", "Aufgaben"),
    f!("Tema", "Theme", "Tema", "Thème", "Design"),
    f!("Terminales", "Terminals", "Terminais", "Terminaux", "Terminals"),
    f!(
        "Todavía no hay ninguno.",
        "There aren't any yet.",
        "Ainda não há nenhum.",
        "Il n'y en a encore aucun.",
        "Noch keine vorhanden.",
    ),
    f!(
        "Todavía no hay ninguno. Hacen falta al menos cuatro memorias del mismo asunto con más de cinco días — la reflexión corre sola cada día, o desde Mantenimiento → Reflexionar ahora.",
        "None yet. It takes at least four memories on the same subject, more than five days apart — reflection runs on its own each day, or from Maintenance → Reflect now.",
        "Ainda não há nenhum. São precisas pelo menos quatro memórias do mesmo assunto com mais de cinco dias — a reflexão corre sozinha todos os dias, ou a partir de Manutenção → Refletir agora.",
        "Il n'y en a encore aucun. Il faut au moins quatre mémoires sur le même sujet, âgées de plus de cinq jours — la réflexion se lance seule chaque jour, ou depuis Maintenance → Réfléchir maintenant.",
        "Noch keiner. Nötig sind mindestens vier Erinnerungen zum selben Thema, älter als fünf Tage — die Reflexion läuft täglich von selbst, oder über Wartung → Jetzt reflektieren.",
    ),
    f!(
        "Todavía no hay ninguno. Salen solos: una conversación con al menos cuatro turnos y tres comandos o lecturas se destila al cerrar el turno.",
        "None yet. They come on their own: a conversation with at least four turns and three commands or reads is distilled when the turn closes.",
        "Ainda não há nenhum. Saem sozinhos: uma conversa com pelo menos quatro turnos e três comandos ou leituras destila-se ao fechar o turno.",
        "Il n'y en a encore aucun. Ils sortent seuls : une conversation d'au moins quatre tours et trois commandes ou lectures se distille à la fin du tour.",
        "Noch keiner. Sie entstehen von selbst: Ein Gespräch mit mindestens vier Runden und drei Befehlen oder Lesevorgängen wird beim Abschluss der Runde destilliert.",
    ),
    f!(
        "Todavía no hay ninguno. También se dictan con /principio.",
        "None yet. You can also dictate them with /principio.",
        "Ainda não há nenhum. Também se ditam com /principio.",
        "Il n'y en a encore aucun. Ils se dictent aussi avec /principio.",
        "Noch keiner. Sie lassen sich auch mit /principio diktieren.",
    ),
    f!("Todos", "All", "Todos", "Tous", "Alle"),
    f!("Top procesos", "Top processes", "Top processos", "Top processus", "Top-Prozesse"),
    f!(
        "Tope de gasto alcanzado",
        "Spend limit reached",
        "Limite de gasto atingido",
        "Limite de dépense atteinte",
        "Ausgabenlimit erreicht",
    ),
    f!(
        "Tope de gasto de la sesión",
        "Session spending cap",
        "Limite de gasto da sessão",
        "Plafond de dépense de la session",
        "Ausgabenlimit der Sitzung",
    ),
    f!(
        "Tope de pasos alcanzado",
        "Step limit reached",
        "Limite de passos atingido",
        "Limite d'étapes atteinte",
        "Schrittlimit erreicht",
    ),
    f!(
        "Tope de pasos seguidos",
        "Consecutive steps cap",
        "Limite de passos seguidos",
        "Plafond d'étapes enchaînées",
        "Limit für Schritte in Folge",
    ),
    f!(
        "Tope de vueltas de herramienta",
        "Tool round limit",
        "Limite de voltas de ferramenta",
        "Limite de tours d'outil",
        "Limit für Tool-Runden",
    ),
    f!("Trace vacío", "Empty trace", "Trace vazio", "Trace vide", "Trace leer"),
    f!("Transcribiendo…", "Transcribing…", "A transcrever…", "Transcription…", "Transkribiere…"),
    f!(
        "Troceando: {n} trozos",
        "Chunking: {n} chunks",
        "A fragmentar: {n} fragmentos",
        "Découpage : {n} fragments",
        "Zerlegen: {n} Blöcke",
    ),
    f!(
        "Trozos de documento",
        "Document chunks",
        "Fragmentos de documento",
        "Fragments de document",
        "Dokumentfragmente",
    ),
    f!(
        "Trozos sin vector",
        "Chunks with no vector",
        "Fragmentos sem vetor",
        "Fragments sans vecteur",
        "Textstücke ohne Vektor",
    ),
    f!("Tu nombre", "Your name", "O teu nome", "Ton nom", "Dein Name"),
    f!(
        "Umbrales de este equipo",
        "Thresholds for this machine",
        "Limiares desta máquina",
        "Seuils de cette machine",
        "Schwellenwerte dieses Rechners",
    ),
    f!(
        "Un comando, o pídemelo en español…   ·   ↑↓ historial",
        "A command, or just ask me in plain English…   ·   ↑↓ history",
        "Um comando, ou pede-mo em português…   ·   ↑↓ histórico",
        "Une commande, ou demande-le-moi en français…   ·   ↑↓ historique",
        "Ein Befehl, oder frag mich auf Deutsch…   ·   ↑↓ Verlauf",
    ),
    f!(
        "Un patrón descartado deja su huella puesta, así que la reflexión de cada noche no puede volver a darlo de alta. Si este número sube deprisa, lo que falla es el agrupado, no los patrones.",
        "A discarded pattern leaves its fingerprint in place, so the nightly reflection can't file it again. If this number climbs fast, what's failing is the grouping, not the patterns.",
        "Um padrão descartado deixa a sua impressão posta, por isso a reflexão de cada noite não pode voltar a dá-lo de alta. Se este número sobe depressa, o que falha é o agrupamento, não os padrões.",
        "Un motif écarté laisse son empreinte en place, si bien que la réflexion de chaque nuit ne peut plus le réinscrire. Si ce nombre grimpe vite, ce qui cloche c'est le regroupement, pas les motifs.",
        "Ein verworfenes Muster lässt seinen Fingerabdruck stehen, sodass die nächtliche Reflexion es nicht erneut anlegen kann. Steigt diese Zahl schnell, liegt der Fehler bei der Gruppierung, nicht bei den Mustern.",
    ),
    f!(
        "Un patrón es lo que se repite entre memorias que nadie escribió juntas. Reencontrarlo lo refuerza: la confianza sube con cada vez.",
        "A pattern is what repeats across memories nobody wrote together. Finding it again reinforces it: confidence rises each time.",
        "Um padrão é o que se repete entre memórias que ninguém escreveu juntas. Reencontrá-lo reforça-o: a confiança sobe de cada vez.",
        "Un motif est ce qui se répète entre des mémoires que personne n'a écrites ensemble. Le retrouver le renforce : la confiance monte à chaque fois.",
        "Ein Muster ist das, was sich zwischen Erinnerungen wiederholt, die niemand zusammen geschrieben hat. Es erneut zu finden verstärkt es: Das Vertrauen steigt mit jedem Mal.",
    ),
    f!(
        "Un principio entra en TODOS los turnos, venga o no al caso — su valor está justo en los turnos donde a nadie se le habría ocurrido recordarlo. Por eso son pocos.",
        "A principle enters EVERY turn, relevant or not — its value lies precisely in the turns where nobody would have thought to recall it. That's why there are few.",
        "Um princípio entra em TODOS os turnos, venha ou não a propósito — o seu valor está justamente nos turnos onde a ninguém teria ocorrido lembrá-lo. Por isso são poucos.",
        "Un principe entre dans TOUS les tours, qu'il soit pertinent ou non — sa valeur est justement dans les tours où personne n'aurait pensé à le rappeler. Voilà pourquoi ils sont peu nombreux.",
        "Ein Prinzip geht in ALLE Runden ein, ob es passt oder nicht — sein Wert liegt genau in den Runden, in denen niemand daran gedacht hätte. Deshalb sind es wenige.",
    ),
    f!(
        "Un servidor de compilación al 90 % está trabajando. Lo que aquí se ajusta cambia el color, las alertas y el indicador de salud — solo en este equipo.",
        "A build server at 90% is just doing its job. What you set here changes the color, the alerts and the health indicator — on this machine only.",
        "Um servidor de compilação a 90 % está a trabalhar. O que aqui se ajusta muda a cor, os alertas e o indicador de saúde — só nesta máquina.",
        "Un serveur de compilation à 90 % est en train de travailler. Ce que tu règles ici change la couleur, les alertes et l'indicateur de santé — uniquement sur cette machine.",
        "Ein Buildserver bei 90 % tut nur seine Arbeit. Was du hier einstellst, ändert Farbe, Warnungen und Zustandsanzeige — nur auf diesem Rechner.",
    ),
    f!(
        "Un skill fijado enmarca todas las respuestas. Se quita con /preset clear.",
        "A pinned skill frames every answer. Remove it with /preset clear.",
        "Um skill fixado enquadra todas as respostas. Tira-se com /preset clear.",
        "Un skill épinglé encadre toutes les réponses. Se retire avec /preset clear.",
        "Ein angehefteter Skill prägt alle Antworten. Entfernen mit /preset clear.",
    ),
    f!(
        "Una PowerShell de verdad: en este equipo, o en uno remoto por WinRM. También \
         acepta que le pidas el comando en español y te lo escribe en la línea para que lo \
         revises antes de soltarlo. Los equipos se dan de alta en el carril de la izquierda.",
        "A real PowerShell: on this machine, or on a remote one over WinRM. It also takes \
         the command asked for in plain words and writes it on the line so you can check it \
         before letting it go. Machines are set up in the left rail.",
        "Uma PowerShell a sério: neste equipamento, ou num remoto por WinRM. Também aceita \
         que lhe peças o comando por palavras e escreve-o na linha para o reveres antes de \
         o largar. Os equipamentos registam-se na barra da esquerda.",
        "Un vrai PowerShell : sur cette machine, ou sur une machine distante via WinRM. Il \
         accepte aussi qu'on lui demande la commande en clair et l'écrit sur la ligne pour \
         que vous la relisiez avant de la lancer. Les machines se déclarent dans le rail de \
         gauche.",
        "Eine echte PowerShell: auf diesem Rechner oder über WinRM auf einem entfernten. Sie \
         nimmt den Befehl auch in Worten entgegen und schreibt ihn in die Zeile, damit du ihn \
         vor dem Absenden prüfst. Rechner werden in der linken Leiste angelegt.",
    ),
    f!(
        "Una foto de lo que este equipo tiene: puertos a la escucha, servicios, software \
         instalado, certificados y tareas programadas. No se mira solo — hay que pulsar \
         Escanear, y hasta entonces los recuentos están en blanco.",
        "A snapshot of what this machine has: listening ports, services, installed software, \
         certificates and scheduled tasks. It does not look on its own — you have to press \
         Scan, and until then the counts are blank.",
        "Uma fotografia do que este equipamento tem: portas à escuta, serviços, software \
         instalado, certificados e tarefas agendadas. Não olha sozinho — tens de carregar em \
         Analisar, e até lá as contagens estão em branco.",
        "Un instantané de ce que contient cette machine : ports en écoute, services, \
         logiciels installés, certificats et tâches planifiées. Il ne regarde pas tout seul — \
         il faut appuyer sur Analyser, et jusque-là les compteurs restent vides.",
        "Eine Momentaufnahme dieses Rechners: offene Ports, Dienste, installierte Software, \
         Zertifikate und geplante Aufgaben. Er schaut nicht von selbst — du musst auf Scannen \
         drücken, bis dahin bleiben die Zahlen leer.",
    ),
    f!(
        "Una pasada en blanco no dice nada; muchas seguidas sí. Suele significar que el corpus no da para agrupar todavía, o que los umbrales de parecido están puestos para otro tamaño de corpus.",
        "One empty pass says nothing; many in a row do. It usually means the corpus isn't big enough to group yet, or that the similarity thresholds are set for a different corpus size.",
        "Uma passagem em branco não diz nada; muitas seguidas dizem. Costuma significar que o corpus ainda não dá para agrupar, ou que os limiares de semelhança estão postos para outro tamanho de corpus.",
        "Un passage à vide ne dit rien ; beaucoup à la suite, si. Cela signifie d'ordinaire que le corpus ne suffit pas encore à regrouper, ou que les seuils de similarité sont réglés pour une autre taille de corpus.",
        "Ein leerer Durchgang sagt nichts; viele hintereinander schon. Meist heißt das, dass der Korpus zum Gruppieren noch nicht reicht, oder dass die Ähnlichkeitsschwellen für eine andere Korpusgröße gesetzt sind.",
    ),
    f!(
        "Usar mi carpeta personal",
        "Use my home folder",
        "Usar a minha pasta pessoal",
        "Utiliser mon dossier personnel",
        "Meinen Benutzerordner verwenden",
    ),
    f!("Usuario", "Username", "Utilizador", "Utilisateur", "Benutzer"),
    f!(
        "Ver cambios",
        "View changes",
        "Ver alterações",
        "Voir les changements",
        "Änderungen ansehen",
    ),
    f!(
        "Ver crystals de memoria",
        "View memory crystals",
        "Ver crystals de memória",
        "Voir les crystals de mémoire",
        "Gedächtnis-Crystals ansehen",
    ),
    f!("Ver detalle", "Show details", "Ver detalhe", "Voir le détail", "Details anzeigen"),
    f!(
        "Ver la evidencia",
        "View the evidence",
        "Ver as provas",
        "Voir la preuve",
        "Nachweis ansehen",
    ),
    f!(
        "Ver la última decisión de routing",
        "See the last routing decision",
        "Ver a última decisão de routing",
        "Voir la dernière décision de routing",
        "Letzte Routing-Entscheidung ansehen",
    ),
    f!("Ver las alertas", "Show alerts", "Ver os alertas", "Voir les alertes", "Warnungen anzeigen"),
    f!("Ver sus logs", "View its logs", "Ver os seus logs", "Voir ses logs", "Logs ansehen"),
    f!("Versión", "Version", "Versão", "Version", "Version"),
    f!("Visor de logs", "Log viewer", "Visor de logs", "Visionneuse de logs", "Log-Ansicht"),
    f!(
        "Volver a abrir el carril del agente",
        "Reopen the agent rail",
        "Voltar a abrir o carril do agente",
        "Rouvrir le rail de l'agent",
        "Die Agenten-Leiste wieder öffnen",
    ),
    // ── La ayuda de cada módulo ──────────────────────────────────────────────
    f!(
        "Volver a los de fábrica",
        "Reset to defaults",
        "Voltar aos de fábrica",
        "Rétablir les valeurs d'usine",
        "Auf Standard zurücksetzen",
    ),
    f!(
        "Volver al inventario",
        "Back to inventory",
        "Voltar ao inventário",
        "Retour à l'inventaire",
        "Zurück zum Inventar",
    ),
    f!(
        "Vulnerabilidades",
        "Vulnerabilities",
        "Vulnerabilidades",
        "Vulnérabilités",
        "Schwachstellen",
    ),
    f!(
        "Windows pedirá confirmación (UAC)",
        "Windows will ask for confirmation (UAC)",
        "O Windows vai pedir confirmação (UAC)",
        "Windows demandera une confirmation (UAC)",
        "Windows fragt nach Bestätigung (UAC)",
    ),
    // ── Botones sueltos ─────────────────────────────────────────────────────
    // Estos eran INVISIBLES para el contador hasta que se le añadió `ui.button`.
    // Salían en español en cualquier idioma, y nadie los echaba de menos porque
    // el número decía que quedaban otros.
    f!(
        "Ya hay una tarea llamada «{id}» corriendo. Recógela con wait_task:{id} o llama a esta de otra forma.",
        "There is already a task named «{id}» running. Pick it up with wait_task:{id}, or name this one differently.",
        "Já existe uma tarefa chamada «{id}» em execução. Recolhe-a com wait_task:{id} ou dá outro nome a esta.",
        "Une tâche nommée «{id}» tourne déjà. Récupère-la avec wait_task:{id} ou donne un autre nom à celle-ci.",
        "Es läuft schon eine Aufgabe namens «{id}». Hol sie mit wait_task:{id} ab oder nenn diese hier anders.",
    ),
    f!(
        "[salida retenida por el guardrail: {motivo}]",
        "[output held by the guardrail: {motivo}]",
        "[saída retida pelo guardrail: {motivo}]",
        "[sortie retenue par le guardrail : {motivo}]",
        "[Ausgabe vom guardrail zurückgehalten: {motivo}]",
    ),
    f!(
        "a partir de qué número avisa y a partir de cuál alarma",
        "at what number it warns and at what it goes critical",
        "a partir de que número avisa e a partir de qual alarma",
        "à partir de quelle valeur elle avertit et à partir de laquelle elle passe en critique",
        "ab welchem Wert gewarnt und ab welchem alarmiert wird",
    ),
    f!("act. {hora}", "upd. {hora}", "atu. {hora}", "maj {hora}", "akt. {hora}"),
    f!("ahora", "now", "agora", "à l'instant", "jetzt"),
    f!("alta", "high", "alta", "élevée", "hoch"),
    // Una sola, y con el punto medio escrito tal cual. Llegó a haber dos —esta y
    // otra con `\u{b7}`— porque en main.rs el punto va escapado y el barrido lo
    // recogió en las dos formas. En ejecución son la MISMA cadena, y dos filas
    // iguales rompen la búsqueda binaria: deja de cumplirse que la tabla esté
    // estrictamente ordenada, y a partir de ahí `busca` puede fallar en
    // cualquier frase, no solo en esta.
    f!(
        "antes de mandar una tarea exigente · no cambia el modelo por ti",
        "before you send a demanding task · doesn't switch the model for you",
        "antes de enviar uma tarefa exigente · não muda o modelo por ti",
        "avant d'envoyer une tâche exigeante · ne change pas le modèle à ta place",
        "vor einer anspruchsvollen Aufgabe · wechselt das Modell nicht für dich",
    ),
    f!(
        "aprender «{nombre}»",
        "learn «{nombre}»",
        "aprender «{nombre}»",
        "apprendre « {nombre} »",
        "«{nombre}» lernen",
    ),
    f!("audit trail", "audit trail", "registo de auditoria", "piste d'audit", "Audit-Trail"),
    f!("baja", "low", "baixa", "faible", "niedrig"),
    f!(
        "baja {pts} pts/día",
        "down {pts} pts/day",
        "desce {pts} pts/dia",
        "baisse de {pts} pts/jour",
        "fällt um {pts} Pkt./Tag",
    ),
    f!(
        "busca patrones entre memorias con más de cinco días",
        "looks for patterns across memories older than five days",
        "procura padrões entre memórias com mais de cinco dias",
        "cherche des motifs entre les mémoires de plus de cinq jours",
        "sucht Muster in Erinnerungen, die älter als fünf Tage sind",
    ),
    f!(
        "busca por lo que quieres decir, no por las palabras exactas",
        "searches by what you mean, not by the exact words",
        "procura pelo que queres dizer, não pelas palavras exatas",
        "cherche par le sens, pas par les mots exacts",
        "sucht nach dem Sinn, nicht nach den genauen Wörtern",
    ),
    f!(
        "buscable por significado",
        "searchable by meaning",
        "pesquisável por significado",
        "recherchable par le sens",
        "nach Bedeutung durchsuchbar",
    ),
    f!(
        "buscando en {ruta}…",
        "searching in {ruta}…",
        "a procurar em {ruta}…",
        "recherche dans {ruta}…",
        "suche in {ruta}…",
    ),
    f!(
        "caducó hace {d}d",
        "expired {d}d ago",
        "caducou há {d}d",
        "expiré il y a {d}j",
        "vor {d}d verfallen",
    ),
    f!("cambió", "changed", "mudou", "modifié", "geändert"),
    f!(
        "comandos encadenados sin aprobar, por orden",
        "chained commands without approval, one after another",
        "comandos encadeados sem aprovação, por ordem",
        "commandes enchaînées sans approbation, par ordre donné",
        "verkettete Befehle ohne Freigabe, der Reihe nach",
    ),
    f!("conectando…", "connecting…", "a ligar…", "connexion…", "verbinde…"),
    f!("configurada", "configured", "configurada", "configuré", "konfiguriert"),
    f!(
        "consistente, aunque Lucy esté escribiendo",
        "consistent, even while Lucy is typing",
        "consistente, mesmo com a Lucy a escrever",
        "cohérent, même si Lucy écrit",
        "konsistent, auch während Lucy schreibt",
    ),
    f!(
        "consolidación: {c}",
        "consolidation: {c}",
        "consolidação: {c}",
        "consolidation : {c}",
        "Konsolidierung: {c}",
    ),
    f!("control nuevo", "new control", "controlo novo", "nouveau contrôle", "neue Prüfung"),
    f!("coste n/d", "cost n/a", "custo n/d", "coût n/d", "Kosten k. A."),
    f!("crítica", "critical", "crítica", "critique", "kritisch"),
    f!(
        "cuánto se extiende al contestar · no cambia qué ejecuta ni qué avisa",
        "how much it elaborates when answering · doesn't change what it runs or warns about",
        "quanto se alonga a responder · não muda o que executa nem o que avisa",
        "combien elle développe ses réponses · ne change ni ce qu'elle exécute ni ce qu'elle signale",
        "wie ausführlich sie antwortet · ändert nicht, was sie ausführt oder meldet",
    ),
    f!(
        "de la interfaz y de lo que Lucy responde · traducidas esta pantalla, la \
         navegación y la ayuda; las demás van en camino",
        "of the interface and of what Lucy answers · this screen, the navigation and \
         the help are translated; the rest are on the way",
        "da interface e do que a Lucy responde · traduzidos este ecrã, a navegação e \
         a ajuda; os restantes vêm a caminho",
        "de l'interface et de ce que Lucy répond · cet écran, la navigation et l'aide \
         sont traduits ; les autres arrivent",
        "der Oberfläche und dessen, was Lucy antwortet · dieser Bildschirm, die \
         Navigation und die Hilfe sind übersetzt; der Rest folgt",
    ),
    f!(
        "de los manuales ingeridos",
        "from the ingested manuals",
        "dos manuais ingeridos",
        "des manuels ingérés",
        "aus den eingelesenen Handbüchern",
    ),
    f!(
        "destila las sesiones y busca lo que se repite",
        "distills sessions and looks for what repeats",
        "destila as sessões e procura o que se repete",
        "distille les sessions et cherche ce qui se répète",
        "destilliert die Sitzungen und sucht, was sich wiederholt",
    ),
    f!("dirección", "address", "endereço", "adresse", "Adresse"),
    f!("editado", "edited", "editado", "édité", "bearbeitet"),
    f!(
        "ejecutando… {s}s",
        "running… {s}s",
        "a executar… {s}s",
        "exécution… {s}s",
        "läuft… {s}s",
    ),
    f!(
        "el globo de Windows se va; esto no",
        "the Windows toast goes away; this doesn't",
        "o balão do Windows desaparece; isto não",
        "la bulle Windows disparaît ; pas ceci",
        "die Windows-Blase verschwindet; das hier nicht",
    ),
    f!(
        "en producción avisa antes de reiniciar un servicio",
        "in production, warn before restarting a service",
        "em produção avisa antes de reiniciar um serviço",
        "en production, prévient avant de redémarrer un service",
        "in der Produktion warnt sie vor dem Neustart eines Dienstes",
    ),
    f!("en vivo · {hora}", "live · {hora}", "ao vivo · {hora}", "en direct · {hora}", "live · {hora}"),
    f!(
        "entran en todos los prompts",
        "they go into every prompt",
        "entram em todos os prompts",
        "présentes dans tous les prompts",
        "gehen in jeden Prompt ein",
    ),
    f!(
        "escaneado {hora}",
        "scanned {hora}",
        "analisado {hora}",
        "analysé {hora}",
        "gescannt {hora}",
    ),
    f!("escaneando… {s}s", "scanning… {s}s", "a analisar… {s}s", "analyse… {s}s", "scanne… {s}s"),
    f!(
        "escribe owner/model",
        "type owner/model",
        "escreve owner/model",
        "écris owner/model",
        "gib owner/model ein",
    ),
    f!(
        "escritas al cerrar un turno",
        "written when a turn closes",
        "escritas ao fechar um turno",
        "écrites en fin de tour",
        "am Ende einer Runde geschrieben",
    ),
    f!("escrito", "written", "escrito", "écrit", "geschrieben"),
    f!(
        "escritura progresiva y transiciones · LUCY_NO_MOTION=1 las apaga al arrancar",
        "progressive typing and transitions · LUCY_NO_MOTION=1 turns them off at startup",
        "escrita progressiva e transições · LUCY_NO_MOTION=1 desliga-as ao arrancar",
        "écriture progressive et transitions · LUCY_NO_MOTION=1 les désactive au démarrage",
        "schrittweise Ausgabe und Übergänge · LUCY_NO_MOTION=1 schaltet sie beim Start aus",
    ),
    f!("estable", "steady", "estável", "stable", "stabil"),
    f!("este equipo", "this machine", "esta máquina", "ce poste", "dieser Rechner"),
    f!("fecha ilegible", "unreadable date", "data ilegível", "date illisible", "Datum unlesbar"),
    f!(
        "fijo, sin seguir a Windows",
        "fixed, does not follow Windows",
        "fixo, sem seguir o Windows",
        "fixe, sans suivre Windows",
        "fest, folgt Windows nicht",
    ),
    f!(
        "fijo. Pensado para pantallas con reflejos; el oscuro es el tema de casa",
        "fixed. Made for screens with glare; dark is the house theme",
        "fixo. Pensado para ecrãs com reflexos; o escuro é o tema da casa",
        "fixe. Conçu pour les écrans avec reflets ; le sombre est le thème maison",
        "fest. Für spiegelnde Bildschirme; Dunkel ist das Standardthema",
    ),
    f!(
        "filtrar por texto — Intro para búsqueda semántica",
        "filter by text — Enter for semantic search",
        "filtrar por texto — Enter para pesquisa semântica",
        "filtrer par texte — Entrée pour la recherche sémantique",
        "nach Text filtern — Enter für semantische Suche",
    ),
    f!(
        "funde memorias que dicen lo mismo; nada se borra",
        "merges memories that say the same thing; nothing is deleted",
        "funde memórias que dizem o mesmo; nada se apaga",
        "fusionne les mémoires qui disent la même chose ; rien n'est supprimé",
        "führt gleichlautende Erinnerungen zusammen; nichts wird gelöscht",
    ),
    f!(
        "fundidas por la consolidación; ya no se leen",
        "merged by consolidation; no longer read",
        "fundidas pela consolidação; já não se leem",
        "fusionnées par la consolidation ; elles ne sont plus lues",
        "von der Konsolidierung zusammengeführt; werden nicht mehr gelesen",
    ),
    f!(
        "ha dejado de cumplir",
        "no longer compliant",
        "deixou de cumprir",
        "n'est plus conforme",
        "erfüllt es nicht mehr",
    ),
    f!("hace un momento", "just now", "há um momento", "à l'instant", "gerade eben"),
    f!("hace {n} d", "{n}d ago", "há {n} d", "il y a {n} j", "vor {n} T."),
    f!("hace {n} días", "{n} days ago", "há {n} dias", "il y a {n} jours", "vor {n} Tagen"),
    f!("hace {n} h", "{n} h ago", "há {n} h", "il y a {n} h", "vor {n} Std."),
    f!("hace {n} min", "{n} min ago", "há {n} min", "il y a {n} min", "vor {n} Min."),
    f!(
        "hechos que Lucy recuerda",
        "facts Lucy remembers",
        "factos que a Lucy recorda",
        "faits dont Lucy se souvient",
        "Fakten, die Lucy sich merkt",
    ),
    f!(
        "hoy {hoy} · 30 días {mes}",
        "today {hoy} · 30 days {mes}",
        "hoje {hoy} · 30 dias {mes}",
        "aujourd'hui {hoy} · 30 jours {mes}",
        "heute {hoy} · 30 Tage {mes}",
    ),
    f!(
        "incluido en el prompt",
        "included in the prompt",
        "incluído no prompt",
        "inclus dans le prompt",
        "im Prompt enthalten",
    ),
    f!(
        "las cifras se comprueban una a una contra la medición; hoy con un modelo pequeño la plantilla suele salir mejor",
        "every figure is checked against the measurement; today, with a small model, the template usually reads better",
        "os números são verificados um a um contra a medição; hoje, com um modelo pequeno, a plantilha costuma sair melhor",
        "chaque chiffre est vérifié face à la mesure ; aujourd'hui, avec un petit modèle, le gabarit se lit mieux",
        "jede Zahl wird gegen die Messung geprüft; heute liest sich mit einem kleinen Modell die Vorlage besser",
    ),
    f!("leyendo… {s}s", "reading… {s}s", "a ler… {s}s", "lecture… {s}s", "lese… {s}s"),
    // ── Servicios detenidos, ya accionables ─────────────────────────────────
    f!(
        "llevas {gastado} · 0 = sin límite",
        "you've spent {gastado} · 0 = no limit",
        "levas {gastado} · 0 = sem limite",
        "vous avez dépensé {gastado} · 0 = sans limite",
        "bisher {gastado} · 0 = kein Limit",
    ),
    f!(
        "llevas {gastado} · al cruzarlo se apaga el automático",
        "you've spent {gastado} · crossing it turns the automatic off",
        "levas {gastado} · ao ultrapassá-lo o automático desliga-se",
        "vous avez dépensé {gastado} · au-delà, l'automatique s'arrête",
        "bisher {gastado} · beim Überschreiten schaltet sich der Automatik-Modus ab",
    ),
    f!(
        "llevas {g} · 0 = sin límite",
        "you are at {g} · 0 = no limit",
        "vais em {g} · 0 = sem limite",
        "tu en es à {g} · 0 = sans limite",
        "bisher {g} · 0 = kein Limit",
    ),
    f!(
        "llevas {g} · al cruzarlo se apaga el automático",
        "you are at {g} · crossing it turns off auto mode",
        "vais em {g} · ao passar disso o automático desliga-se",
        "tu en es à {g} · au-delà, l'automatique s'arrête",
        "bisher {g} · beim Überschreiten schaltet sich die Automatik ab",
    ),
    f!(
        "lo que se ilumina: navegación, progreso, hecho",
        "what lights up: navigation, progress, done",
        "o que se ilumina: navegação, progresso, concluído",
        "ce qui s'allume : navigation, progression, terminé",
        "was hervorgehoben wird: Navigation, Fortschritt, erledigt",
    ),
    f!(
        "lo que se repite entre memorias",
        "what repeats across memories",
        "o que se repete entre memórias",
        "ce qui se répète d'une mémoire à l'autre",
        "was sich in den Erinnerungen wiederholt",
    ),
    f!(
        "los guardrails que revisan la credencial antes de usarla.",
        "the guardrails that check the credential before it is used.",
        "guardrails que verificam a credencial antes de a usar.",
        "les guardrails qui vérifient l'identifiant avant de l'utiliser.",
        "den Guardrails, die die Anmeldedaten vor dem Einsatz prüfen.",
    ),
    f!("media", "medium", "média", "moyenne", "mittel"),
    f!("memoria", "memory", "memória", "mémoire", "Gedächtnis"),
    f!("modo {p}", "{p} mode", "modo {p}", "mode {p}", "Modus {p}"),
    f!("nada pendiente", "nothing pending", "nada pendente", "rien en attente", "nichts offen"),
    f!("no responde", "not responding", "não responde", "ne répond pas", "antwortet nicht"),
    f!("no vale", "not valid", "não serve", "invalide", "ungültig"),
    f!("nombre", "name", "nome", "nom", "Name"),
    f!("nuevo", "new", "novo", "nouveau", "neu"),
    f!("pegar clave", "paste key", "colar chave", "coller la clé", "Schlüssel einfügen"),
    f!(
        "por defecto {puerto}",
        "default {puerto}",
        "por omissão {puerto}",
        "{puerto} par défaut",
        "standardmäßig {puerto}",
    ),
    f!("privado", "private", "privado", "privé", "privat"),
    f!(
        "próxima en {plazo}",
        "next in {plazo}",
        "próxima em {plazo}",
        "prochaine dans {plazo}",
        "nächste in {plazo}",
    ),
    f!("reflexión: {r}", "reflection: {r}", "reflexão: {r}", "réflexion : {r}", "Reflexion: {r}"),
    f!(
        "se llena en ~{dias} días",
        "full in ~{dias} days",
        "enche em ~{dias} dias",
        "plein dans ~{dias} jours",
        "voll in ~{dias} Tagen",
    ),
    f!(
        "si se deja vacío usa el usuario de Windows, que es una cuenta y no un nombre",
        "if left empty it uses the Windows user, which is an account and not a name",
        "se ficar vazio usa o utilizador do Windows, que é uma conta e não um nome",
        "si tu le laisses vide, on prend l'utilisateur Windows, qui est un compte et pas un nom",
        "wenn leer, gilt der Windows-Benutzer, und das ist ein Konto, kein Name",
    ),
    f!(
        "sigue a Windows — mira el ajuste de las APLICACIONES, no el de la barra de tareas: mucha gente los tiene cruzados",
        "follows Windows — check the APPS setting, not the taskbar one: plenty of people have them mismatched",
        "segue o Windows — vê a definição das APLICAÇÕES, não a da barra de tarefas: muita gente tem-nas trocadas",
        "suit Windows — regarde le réglage des APPLICATIONS, pas celui de la barre des tâches : beaucoup de gens les ont inversés",
        "folgt Windows — sieh in der Einstellung für APPS nach, nicht in der für die Taskleiste: bei vielen stehen die überkreuz",
    ),
    f!(
        "sin actividad en 30 días",
        "no activity in 30 days",
        "sem atividade em 30 dias",
        "aucune activité en 30 jours",
        "keine Aktivität in 30 Tagen",
    ),
    f!("sin clave", "no key", "sem chave", "sans clé", "ohne Schlüssel"),
    f!(
        "sin dirección aún",
        "no address yet",
        "sem endereço ainda",
        "pas encore d’adresse",
        "noch keine Adresse",
    ),
    f!("sin etiqueta", "no label", "sem etiqueta", "sans étiquette", "ohne Label"),
    f!("sin migrar", "not migrated", "por migrar", "non migré", "nicht migriert"),
    f!(
        "sin modelo de texto no se destila ninguna sesión",
        "without a text model no session gets distilled",
        "sem modelo de texto não se destila nenhuma sessão",
        "sans modèle de texte, aucune session n'est distillée",
        "ohne Textmodell wird keine Sitzung destilliert",
    ),
    f!("sin saber", "unknown", "por saber", "inconnue", "ungeprüft"),
    f!(
        "sin él, Lucy recuerda solo por palabras y encuentra bastante menos",
        "without it, Lucy remembers by words alone and finds far less",
        "sem ele, a Lucy lembra-se só por palavras e encontra bastante menos",
        "sans lui, Lucy ne se souvient que par mots et trouve bien moins",
        "ohne ihn erinnert sich Lucy nur über Wörter und findet deutlich weniger",
    ),
    f!(
        "solo se encuentran por palabras — pasó si Ollama estaba caído al ingerir",
        "only found by words — happened if Ollama was down at ingest",
        "só se encontram por palavras — aconteceu se o Ollama estava em baixo ao ingerir",
        "on ne les trouve que par mots — arrive si Ollama était en panne à l'ingestion",
        "nur über Wörter auffindbar — passiert, wenn Ollama beim Einlesen aus war",
    ),
    f!(
        "sube {pts} pts/día",
        "up {pts} pts/day",
        "sobe {pts} pts/dia",
        "monte de {pts} pts/jour",
        "steigt um {pts} Pkt./Tag",
    ),
    f!(
        "todavía no hay bastante para una tendencia",
        "not enough yet for a trend",
        "ainda não chega para uma tendência",
        "pas encore assez de données pour une tendance",
        "noch zu wenig für einen Trend",
    ),
    f!(
        "todavía no hay nada apuntado en esta base",
        "nothing recorded in this database yet",
        "ainda não há nada apontado nesta base",
        "rien n'est encore enregistré dans cette base",
        "in dieser Datenbank ist noch nichts erfasst",
    ),
    f!(
        "todavía vive en src-tauri, junto al transporte WinRM y a",
        "still lives in src-tauri, alongside the WinRM transport and",
        "ainda vive em src-tauri, junto ao transporte WinRM e aos",
        "vit encore dans src-tauri, avec le transport WinRM et",
        "lebt noch in src-tauri, neben dem WinRM-Transport und",
    ),
    f!(
        "todo el tráfico a Ollama local",
        "all traffic to local Ollama",
        "todo o tráfego para o Ollama local",
        "tout le trafic vers Ollama local",
        "aller Datenverkehr zum lokalen Ollama",
    ),
    f!("traduciendo…", "translating…", "a traduzir…", "traduction…", "übersetze…"),
    f!("un momento", "one moment", "um momento", "un instant", "einen Moment"),
    f!("usuario", "username", "utilizador", "utilisateur", "Benutzer"),
    f!(
        "vacío = ssh-agent o ~/.ssh/id_ed25519",
        "empty = ssh-agent or ~/.ssh/id_ed25519",
        "vazio = ssh-agent ou ~/.ssh/id_ed25519",
        "vide = ssh-agent ou ~/.ssh/id_ed25519",
        "leer = ssh-agent oder ~/.ssh/id_ed25519",
    ),
    f!(
        "vencido: correrá en la próxima comprobación",
        "overdue: runs at the next check",
        "vencido: correrá na próxima verificação",
        "échu : s'exécutera à la prochaine vérification",
        "überfällig: läuft bei der nächsten Prüfung",
    ),
    f!("visto 1 vez", "seen once", "visto 1 vez", "vu 1 fois", "1-mal gesehen"),
    f!("visto {n} veces", "seen {n} times", "visto {n} vezes", "vu {n} fois", "{n}-mal gesehen"),
    f!(
        "vuelve a contar lo de arriba",
        "counts everything above again",
        "volta a contar o de cima",
        "recompte ce qu'il y a au-dessus",
        "zählt die Werte oben neu",
    ),
    f!("vuelve a medirse", "measurable again", "volta a medir-se", "de nouveau mesuré", "wird wieder gemessen"),
    f!("válida", "valid", "válida", "valide", "gültig"),
    f!("ya cumple", "now compliant", "já cumpre", "conforme désormais", "erfüllt es jetzt"),
    f!("ya no está", "gone", "já não está", "n'existe plus", "nicht mehr da"),
    f!(
        "ya no se puede medir",
        "can no longer be measured",
        "já não se pode medir",
        "ne peut plus être mesuré",
        "lässt sich nicht mehr messen",
    ),
    f!(
        "{activos} de {n_skills} activos. Los apagados siguen en disco y no entran en lo que Lucy ve, así que deja de pedirlos. Se instalan en tu perfil y sobreviven a reinstalar Lucy.",
        "{activos} of {n_skills} active. The ones turned off stay on disk and are outside what Lucy sees, so she stops asking for them. They install to your profile and survive reinstalling Lucy.",
        "{activos} de {n_skills} ativas. As desligadas continuam em disco e não entram no que a Lucy vê, por isso deixa de as pedir. Instalam-se no teu perfil e sobrevivem a reinstalar a Lucy.",
        "{activos} actifs sur {n_skills}. Ceux qui sont désactivés restent sur le disque et n'entrent pas dans ce que Lucy voit, donc elle arrête de les demander. Ils s'installent dans ton profil et survivent à une réinstallation de Lucy.",
        "{activos} von {n_skills} aktiv. Die ausgeschalteten bleiben auf der Platte, aber Lucy sieht sie nicht mehr und ruft sie nicht mehr auf. Sie liegen in deinem Profil und überstehen eine Neuinstallation von Lucy.",
    ),
    f!(
        "{caducados} pasos sin aprobar caducan",
        "{caducados} unapproved steps expire",
        "{caducados} passos por aprovar caducam",
        "{caducados} étapes non approuvées expirent",
        "{caducados} nicht genehmigte Schritte verfallen",
    ),
    f!("{cat} {n}", "{cat} {n}", "{cat} {n}", "{cat} {n}", "{cat} {n}"),
    f!(
        "{cat}: {motivo}",
        "{cat}: {motivo}",
        "{cat}: {motivo}",
        "{cat} : {motivo}",
        "{cat}: {motivo}",
    ),
    f!(
        "{con} de {total} con vector — el resto solo se encuentra por palabras",
        "{con} of {total} with a vector — the rest is only found by keyword",
        "{con} de {total} com vetor — o resto só se encontra por palavras",
        "{con} sur {total} avec vecteur — le reste ne se trouve que par mots-clés",
        "{con} von {total} mit Vektor — der Rest wird nur über Wörter gefunden",
    ),
    f!(
        "{crashed} servicio(s) con fallo de arranque",
        "{crashed} service(s) failed to start",
        "{crashed} serviço(s) com falha de arranque",
        "{crashed} service(s) en échec de démarrage",
        "{crashed} Dienst(e) mit Startfehler",
    ),
    f!(
        "{ent} tokens de entrada, {sal} de salida en esta terminal",
        "{ent} input tokens, {sal} output in this terminal",
        "{ent} tokens de entrada, {sal} de saída neste terminal",
        "{ent} tokens en entrée, {sal} en sortie dans ce terminal",
        "{ent} Tokens Eingabe, {sal} Ausgabe in diesem Terminal",
    ),
    f!(
        "{grupos} grupos fundidos · {memorias} memorias marcadas. No se borró ninguna: quedan etiquetadas y fuera de las consultas vivas.",
        "{grupos} groups merged · {memorias} memories flagged. None were deleted: they stay tagged and out of live queries.",
        "{grupos} grupos fundidos · {memorias} memórias marcadas. Não se apagou nenhuma: ficam etiquetadas e fora das consultas vivas.",
        "{grupos} groupes fusionnés · {memorias} mémoires marquées. Aucune n'a été supprimée : elles restent étiquetées et hors des requêtes actives.",
        "{grupos} Gruppen zusammengeführt · {memorias} Erinnerungen markiert. Gelöscht wurde keine: Sie bleiben markiert und außerhalb der aktiven Abfragen.",
    ),
    f!(
        "{grupos} grupos · {memorias} memorias se fundirían en otra, de {miradas} miradas. No se ha tocado nada todavía.",
        "{grupos} groups · {memorias} memories would merge into another, out of {miradas} looked at. Nothing has been touched yet.",
        "{grupos} grupos · {memorias} memórias seriam fundidas noutra, de {miradas} vistas. Ainda não se tocou em nada.",
        "{grupos} groupes · {memorias} mémoires fusionneraient dans une autre, sur {miradas} examinées. Rien n'a encore été modifié.",
        "{grupos} Gruppen · {memorias} Erinnerungen würden zu einer verschmelzen, aus {miradas} Sichtungen. Es wurde noch nichts verändert.",
    ),
    f!(
        "{h} h encendido",
        "{h} h up",
        "{h} h ligado",
        "{h} h allumé",
        "{h} h in Betrieb",
    ),
    f!(
        "{libre} libres de {total}",
        "{libre} free of {total}",
        "{libre} livres de {total}",
        "{libre} libres sur {total}",
        "{libre} frei von {total}",
    ),
    f!(
        "{libre} libres · {usado} / {total}",
        "{libre} free · {usado} / {total}",
        "{libre} livres · {usado} / {total}",
        "{libre} libres · {usado} / {total}",
        "{libre} frei · {usado} / {total}",
    ),
    // ── Plantillas con hueco ────────────────────────────────────────────────
    // El hueco lleva NOMBRE y cambia de sitio entre idiomas — que es justo por
    // lo que existe `trf`. Ver el test que comprueba que ninguna traducción se
    // deja uno por el camino.
    f!(
        "{max} pasos seguidos sin llegar a una respuesta. El automático se apaga y el siguiente paso lo apruebas tú.",
        "{max} steps in a row without reaching an answer. Auto mode turns off and you approve the next step.",
        "{max} passos seguidos sem chegar a uma resposta. O automático desliga-se e o passo seguinte aprova-lo tu.",
        "{max} étapes d'affilée sans arriver à une réponse. Le mode automatique se désactive et c'est toi qui approuves l'étape suivante.",
        "{max} Schritte hintereinander ohne Antwort. Der Automatikmodus geht aus, den nächsten Schritt gibst du selbst frei.",
    ),
    f!(
        "{max} vueltas pidiendo ficheros sin llegar a una respuesta. El turno vuelve a ti; lo que se leyó está en este mismo carril.",
        "{max} rounds asking for files without reaching an answer. The turn goes back to you; what was read is in this same lane.",
        "{max} voltas a pedir ficheiros sem chegar a uma resposta. O turno volta para ti; o que foi lido está nesta mesma faixa.",
        "{max} tours à demander des fichiers sans arriver à une réponse. Le tour te revient ; ce qui a été lu est dans ce même fil.",
        "{max} Runden Dateiabfragen ohne Antwort. Du bist wieder dran; das Gelesene steht in derselben Spur.",
    ),
    f!(
        "{motivo}. Aprueba el paso para seguir.",
        "{motivo}. Approve the step to continue.",
        "{motivo}. Aprova o passo para continuar.",
        "{motivo}. Approuve l'étape pour continuer.",
        "{motivo}. Gib den Schritt frei, um weiterzumachen.",
    ),
    f!(
        "{ms} ms · {n} caracteres de salida",
        "{ms} ms · {n} characters of output",
        "{ms} ms · {n} caracteres de saída",
        "{ms} ms · {n} caractères de sortie",
        "{ms} ms · {n} Zeichen Ausgabe",
    ),
    f!(
        "{n_claves} de {total}",
        "{n_claves} of {total}",
        "{n_claves} de {total}",
        "{n_claves} sur {total}",
        "{n_claves} von {total}",
    ),
    f!("{n} días", "{n} days", "{n} dias", "{n} jours", "{n} Tage"),
    f!(
        "{n} ficheros en {dir} — el más reciente primero",
        "{n} files in {dir} — newest first",
        "{n} ficheiros em {dir} — o mais recente primeiro",
        "{n} fichiers dans {dir} — le plus récent en premier",
        "{n} Dateien in {dir} — die neueste zuerst",
    ),
    f!("{n} h", "{n} h", "{n} h", "{n} h", "{n} Std."),
    f!(
        "{n} llamadas al modelo en 30 días · {ent} tokens de entrada, {sal} de salida",
        "{n} model calls in 30 days · {ent} input tokens, {sal} output",
        "{n} chamadas ao modelo em 30 dias · {ent} tokens de entrada, {sal} de saída",
        "{n} appels au modèle en 30 jours · {ent} tokens en entrée, {sal} en sortie",
        "{n} Modellaufrufe in 30 Tagen · {ent} Eingabe-Tokens, {sal} Ausgabe",
    ),
    f!(
        "{n} memorias detrás",
        "{n} memories behind it",
        "{n} memórias por trás",
        "{n} mémoires derrière",
        "{n} Erinnerungen dahinter",
    ),
    f!("{n} min", "{n} min", "{n} min", "{n} min", "{n} Min."),
    f!(
        "{n} muestras desde hace {plazo}",
        "{n} samples over the last {plazo}",
        "{n} amostras desde há {plazo}",
        "{n} mesures depuis {plazo}",
        "{n} Messwerte seit {plazo}",
    ),
    f!("{n} núcleos", "{n} cores", "{n} núcleos", "{n} cœurs", "{n} Kerne"),
    f!(
        "{n} patrones descartados — no volverán",
        "{n} patterns discarded — they won't come back",
        "{n} padrões descartados — não voltarão",
        "{n} motifs écartés — ils ne reviendront pas",
        "{n} Muster verworfen — sie kommen nicht zurück",
    ),
    // SINGULAR Y PLURAL COMO DOS FRASES: en alemán el plural de «Laufwerk» es
    // «Laufwerke» y en francés cambia el artículo. Pegar una «s» al final solo
    // funciona en español.
    f!(
        "{n} por similitud",
        "{n} by similarity",
        "{n} por semelhança",
        "{n} par similarité",
        "{n} nach Ähnlichkeit",
    ),
    f!("{n} trozos", "{n} chunks", "{n} fragmentos", "{n} fragments", "{n} Fragmente"),
    f!(
        "{n} trozos vuelven a ser buscables por significado.",
        "{n} chunks are searchable by meaning again.",
        "{n} fragmentos voltam a ser pesquisáveis por significado.",
        "{n} fragments sont à nouveau recherchables par sens.",
        "{n} Fragmente sind wieder nach Bedeutung durchsuchbar.",
    ),
    f!("{n} volúmenes", "{n} volumes", "{n} volumes", "{n} volumes", "{n} Laufwerke"),
    f!(
        "{paso} — sin migrar a este shell",
        "{paso} — not migrated to this shell",
        "{paso} — sem migrar para esta shell",
        "{paso} — non migré vers ce shell",
        "{paso} — nicht zu dieser Shell migriert",
    ),
    f!(
        "{pct}% de lo propuesto se ejecutó",
        "{pct}% of what was proposed ran",
        "{pct}% do proposto executou-se",
        "{pct}% de ce qui a été proposé a été exécuté",
        "{pct}% des Vorgeschlagenen lief",
    ),
    f!(
        "{pct}% supervisado",
        "{pct}% supervised",
        "{pct}% supervisionado",
        "{pct}% supervisé",
        "{pct}% überwacht",
    ),
    f!(
        "{pct}% · {usado} de {total} GB",
        "{pct}% · {usado} of {total} GB",
        "{pct}% · {usado} de {total} GB",
        "{pct} % · {usado} sur {total} Go",
        "{pct}% · {usado} von {total} GB",
    ),
    f!(
        "{pista} · el proveedor la acepta",
        "{pista} · the provider accepts it",
        "{pista} · o fornecedor aceita-a",
        "{pista} · le fournisseur l'accepte",
        "{pista} · der Anbieter akzeptiert ihn",
    ),
    f!(
        "{pista} · sin comprobar: {m}",
        "{pista} · unchecked: {m}",
        "{pista} · sem verificar: {m}",
        "{pista} · non vérifiés : {m}",
        "{pista} · ungeprüft: {m}",
    ),
    f!(
        "{usado} de {total} MB",
        "{usado} of {total} MB",
        "{usado} de {total} MB",
        "{usado} sur {total} Mo",
        "{usado} von {total} MB",
    ),
    // Identificadores y un separador: no hay prosa que traducir, pero sí una
    // FORMA que puede cambiar — el francés separa con espacio fino antes del
    // punto medio, y un idioma que escribiera «máquina/usuario» al revés tendría
    // dónde decirlo. Está en la tabla por eso, no para maquillar el recuento.
    f!(
        "{usuario}@{maquina} · {via}",
        "{usuario}@{maquina} · {via}",
        "{usuario}@{maquina} · {via}",
        "{usuario}@{maquina} · {via}",
        "{usuario}@{maquina} · {via}",
    ),
    f!(
        "{vivas} de {total} memorias vivas",
        "{vivas} of {total} live memories",
        "{vivas} de {total} memórias vivas",
        "{vivas} sur {total} mémoires vivantes",
        "{vivas} von {total} lebenden Erinnerungen",
    ),
    f!(
        "«{nombre}» ingerido: {trozos} trozos, todos con vector.",
        "«{nombre}» ingested: {trozos} chunks, all with a vector.",
        "«{nombre}» ingerido: {trozos} fragmentos, todos com vetor.",
        "«{nombre}» ingéré : {trozos} fragments, tous avec vecteur.",
        "«{nombre}» eingelesen: {trozos} Fragmente, alle mit Vektor.",
    ),
    f!(
        "«{nombre}» quedó buscable por palabras ({hechos} de {total} con vector): {e}",
        "«{nombre}» ended up searchable by keyword ({hechos} of {total} with a vector): {e}",
        "«{nombre}» ficou pesquisável por palavras ({hechos} de {total} com vetor): {e}",
        "«{nombre}» est resté recherchable par mots ({hechos} sur {total} avec vecteur) : {e}",
        "«{nombre}» ist per Wortsuche auffindbar ({hechos} von {total} mit Vektor): {e}",
    ),
    f!(
        "· de ellas, automáticas",
        "· of those, automatic",
        "· delas, automáticas",
        "· dont automatiques",
        "· davon automatisch",
    ),
    f!(
        "· desde hace {plazo}",
        "· for the last {plazo}",
        "· desde há {plazo}",
        "· depuis {plazo}",
        "· seit {plazo}",
    ),
    f!("· en uso", "· in use", "· em uso", "· utilisé", "· in Gebrauch"),
    f!(
        "· {n} dinámicos ignorados",
        "· {n} dynamic ones ignored",
        "· {n} dinâmicos ignorados",
        "· {n} dynamiques ignorés",
        "· {n} dynamische ignoriert",
    ),
    f!(
        "¿Borrar {nombre}?",
        "Delete {nombre}?",
        "Eliminar {nombre}?",
        "Supprimer {nombre} ?",
        "{nombre} löschen?",
    ),
    f!(
        "¿Qué servicios de inicio automático están detenidos ahora mismo? Muéstramelos.",
        "Which auto-start services are stopped right now? Show them.",
        "Que serviços de arranque automático estão parados neste momento? Mostra-mos.",
        "Quels services à démarrage automatique sont arrêtés en ce moment ? Montre-les-moi.",
        "Welche Dienste mit Autostart sind gerade gestoppt? Zeig sie mir.",
    ),
    f!(
        "¿Qué ves en mi pantalla? ",
        "What do you see on my screen? ",
        "O que vês no meu ecrã? ",
        "Que vois-tu sur mon écran ? ",
        "Was siehst du auf meinem Bildschirm? ",
    ),
    f!("¿borrar?", "delete?", "apagar?", "supprimer ?", "löschen?"),
    f!(
        "Última vez {cuando} · {plazo}",
        "Last time {cuando} · {plazo}",
        "Última vez {cuando} · {plazo}",
        "Dernière fois {cuando} · {plazo}",
        "Zuletzt {cuando} · {plazo}",
    ),
    f!("Última vuelta", "Last lap", "Última volta", "Dernier tour", "Letzte Runde"),
    f!(
        "Últimos 30 días: {apr} comandos los aprobó una persona, {solos} los lanzó el automático, {desc} se propusieron y no se ejecutaron.",
        "Last 30 days: {apr} commands were approved by a person, {solos} were launched by auto mode, {desc} were proposed and never ran.",
        "Últimos 30 dias: {apr} comandos foram aprovados por uma pessoa, {solos} lançou-os o automático, {desc} foram propostos e não se executaram.",
        "30 derniers jours : {apr} commandes ont été approuvées par une personne, {solos} lancées par le mode automatique, {desc} proposées et jamais exécutées.",
        "Letzte 30 Tage: {apr} Befehle hat eine Person freigegeben, {solos} hat der Automatikmodus gestartet, {desc} wurden vorgeschlagen und liefen nie.",
    ),
    f!(
        "… y {sobran} líneas más",
        "… and {sobran} more lines",
        "… e mais {sobran} linhas",
        "… et {sobran} lignes de plus",
        "… und {sobran} weitere Zeilen",
    ),
    f!("↻ Recargar", "↻ Reload", "↻ Recarregar", "↻ Recharger", "↻ Neu laden"),
    f!("↻ Recontar", "↻ Recount", "↻ Recontar", "↻ Recompter", "↻ Neu zählen"),
    f!("↻ Redetectar", "↻ Redetect", "↻ Redetetar", "↻ Redétecter", "↻ Neu erkennen"),
    f!("↻ Reintentar", "↻ Retry", "↻ Tentar de novo", "↻ Réessayer", "↻ Erneut versuchen"),
    f!("↻ redetectar", "↻ re-detect", "↻ redetetar", "↻ redétecter", "↻ neu erkennen"),
    f!(
        "⇈ Reintentar como administrador",
        "⇈ Retry as administrator",
        "⇈ Repetir como administrador",
        "⇈ Réessayer en administrateur",
        "⇈ Als Administrator wiederholen",
    ),
    f!(
        "⌕   Filtrar {cat}…",
        "⌕   Filter {cat}…",
        "⌕   Filtrar {cat}…",
        "⌕   Filtrer {cat}…",
        "⌕   {cat} filtern…",
    ),
    f!(
        "⌕  Filtrar mensajes…",
        "⌕  Filter messages…",
        "⌕  Filtrar mensagens…",
        "⌕  Filtrer les messages…",
        "⌕  Nachrichten filtern…",
    ),
    f!(
        "⏸ pausado · {hora}",
        "⏸ paused · {hora}",
        "⏸ em pausa · {hora}",
        "⏸ en pause · {hora}",
        "⏸ pausiert · {hora}",
    ),
    f!("■  Parar", "■  Stop", "■  Parar", "■  Arrêter", "■  Anhalten"),
    f!("■ Detener", "■ Stop", "■ Parar", "■ Arrêter", "■ Stopp"),
    f!("▸ Ejecutar", "▸ Run", "▸ Executar", "▸ Exécuter", "▸ Ausführen"),
    f!("◆ {n} sin leer", "◆ {n} unread", "◆ {n} por ler", "◆ {n} non lues", "◆ {n} ungelesen"),
    f!("◈ Semántica", "◈ Semantic", "◈ Semântica", "◈ Sémantique", "◈ Semantisch"),
    f!("● ESCANEADO {hora}", "● SCANNED {hora}", "● ANALISADO {hora}", "● ANALYSÉ {hora}", "● GESCANNT {hora}"),
    f!(
        "⚠ {avisos} avisos",
        "⚠ {avisos} warnings",
        "⚠ {avisos} avisos",
        "⚠ {avisos} avertissements",
        "⚠ {avisos} Warnungen",
    ),
    f!(
        "⚠ {cat}: se enseñan {vistas} de {total}. Una lista recortada en silencio se lee como una lista completa.",
        "⚠ {cat}: showing {vistas} of {total}. A list trimmed in silence reads like a complete one.",
        "⚠ {cat}: mostram-se {vistas} de {total}. Uma lista cortada em silêncio lê-se como uma lista completa.",
        "⚠ {cat} : affichage de {vistas} sur {total}. Une liste tronquée en silence se lit comme une liste complète.",
        "⚠ {cat}: {vistas} von {total} werden angezeigt. Eine still gekürzte Liste liest sich wie eine vollständige.",
    ),
    f!(
        "⚠ {criticas} críticas · {avisos} avisos",
        "⚠ {criticas} critical · {avisos} warnings",
        "⚠ {criticas} críticas · {avisos} avisos",
        "⚠ {criticas} critiques · {avisos} avertissements",
        "⚠ {criticas} kritisch · {avisos} Warnungen",
    ),
    f!(
        "⚠ {sin} de {total} no se pudieron medir y quedan fuera del porcentaje — el motivo \
         está en cada fila.",
        "⚠ {sin} of {total} could not be measured and are left out of the percentage — the \
         reason is on each row.",
        "⚠ {sin} de {total} não se puderam medir e ficam fora da percentagem — o motivo está \
         em cada linha.",
        "⚠ {sin} sur {total} n'ont pas pu être mesurés et restent hors du pourcentage — la \
         raison figure sur chaque ligne.",
        "⚠ {sin} von {total} konnten nicht gemessen werden und zählen nicht zum Prozentsatz — \
         der Grund steht in jeder Zeile.",
    ),
    // Severidad y estado: vienen de `lucy-core`, que no sabe de idiomas, y se
    // traducen en el punto de uso. «OK» se queda igual en los cinco.
    f!("⛨  Escanear", "⛨  Scan", "⛨  Analisar", "⛨  Analyser", "⛨  Scannen"),
    f!(
        "✓ Todos los servicios automáticos en ejecución",
        "✓ All automatic services running",
        "✓ Todos os serviços automáticos em execução",
        "✓ Tous les services automatiques fonctionnent",
        "✓ Alle automatischen Dienste laufen",
    ),
    f!("⟳  Escanear", "⟳  Scan", "⟳  Analisar", "⟳  Analyser", "⟳  Scannen"),
    f!("＋ Añadir", "＋ Add", "＋ Adicionar", "＋ Ajouter", "＋ Hinzufügen"),
    // ── Compliance ──────────────────────────────────────────────────────────
    f!(
        "＋ Ingerir documento",
        "＋ Ingest document",
        "＋ Ingerir documento",
        "＋ Ingérer un document",
        "＋ Dokument einlesen",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Los tests tocan un global. En paralelo se pisan, igual que `motion()`.
    fn cerrojo() -> std::sync::MutexGuard<'static, ()> {
        static M: std::sync::Mutex<()> = std::sync::Mutex::new(());
        M.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn el_lector_de_literales_entiende_las_dos_clases_de_salto() {
        // ESTE REPOSITORIO TIENE `core.autocrlf=true`. En cuanto git toca
        // main.rs las líneas pasan a CRLF, y entonces una barra de continuación
        // ya no va seguida de `\n` sino de `\r\n`. Los lectores exigían `\n`
        // pegado, así que en un clon recién hecho en Windows —la máquina de
        // destino— TODOS los tests de traducción fallaban, y fallaban diciendo
        // que veintidós frases estaban sin traducir cuando lo estaban.
        //
        // Se descubrió por accidente, al restaurar el fichero con `git checkout`
        // en mitad de otra cosa. Sin este test, el siguiente en clonar el
        // repositorio lo habría descubierto igual de por casualidad.
        let esperado = "una frase partida en dos";
        for (nombre, fuente) in [
            ("LF", "\"una frase \\\n         partida en dos\""),
            ("CRLF", "\"una frase \\\r\n         partida en dos\""),
        ] {
            let (v, _) = literal_desde(fuente, 0).expect("tiene que leerse");
            assert_eq!(v, esperado, "con saltos {nombre} el literal se lee mal");
        }
    }

    #[test]
    fn la_tabla_esta_ordenada_porque_la_busqueda_es_binaria() {
        // Una frase fuera de orden no falla: hace que ESA no se encuentre nunca
        // y salga en español para siempre, sin que nada lo diga.
        for par in FRASES.windows(2) {
            // Repetida y descolocada rompen lo mismo pero se arreglan distinto,
            // y con un solo mensaje para las dos se pierde un rato averiguando
            // cuál de las dos es. Una repetida no se ve leyendo la tabla: pasa
            // cuando la misma frase entra dos veces escrita de dos formas —el
            // punto medio como «·» y como `\u{b7}`— y al compilar son iguales.
            assert_ne!(
                par[0].es, par[1].es,
                "«{}» está DOS VECES en la tabla; quita una",
                hasta(par[0].es, 50)
            );
            assert!(
                par[0].es < par[1].es,
                "«{}» va después de «{}» y la tabla tiene que ir ordenada",
                hasta(par[0].es, 40),
                hasta(par[1].es, 40)
            );
        }
    }

    #[test]
    fn ninguna_frase_se_queda_a_medio_traducir() {
        // Una frase que está en la tabla con columnas vacías es peor que no
        // estar: parece traducida al mirar la tabla y sale en español al usarla.
        for f in FRASES {
            for (i, s) in f.otros.iter().enumerate() {
                assert!(
                    !s.trim().is_empty(),
                    "«{}» no tiene {}",
                    &f.es[..f.es.len().min(40)],
                    Lang::ALL[i + 1].nombre()
                );
            }
        }
    }

    #[test]
    fn una_frase_sin_traducir_sale_en_español_y_no_rota() {
        // La propiedad que hace viable convertir por pantallas.
        let _g = cerrojo();
        set(Lang::De);
        assert_eq!(tr("esto no está en la tabla"), "esto no está en la tabla");
        set(Lang::Es);
    }

    #[test]
    fn cambiar_de_idioma_cambia_lo_que_se_lee() {
        let _g = cerrojo();
        set(Lang::Es);
        assert_eq!(tr("Configuración"), "Configuración");
        set(Lang::De);
        assert_eq!(tr("Configuración"), "Einstellungen");
        set(Lang::Fr);
        assert_eq!(tr("Configuración"), "Réglages");
        set(Lang::Es);
    }

    #[test]
    fn el_codigo_de_la_v1_se_entiende_por_prefijo() {
        // La V1 guarda `es-MX` y `en-US`, no `es` y `en`. Un `==` exacto haría
        // que la elección hecha allí se leyera como «no lo sé».
        assert_eq!(Lang::de_clave("es-MX"), Some(Lang::Es));
        assert_eq!(Lang::de_clave("en-US"), Some(Lang::En));
        assert_eq!(Lang::de_clave("pt-BR"), Some(Lang::Pt));
        assert_eq!(Lang::de_clave("FR"), Some(Lang::Fr));
        assert_eq!(Lang::de_clave("de-AT"), Some(Lang::De));
        assert_eq!(Lang::de_clave("ja"), None);
        assert_eq!(Lang::de_clave(""), None);
    }

    /// El fuente de la pantalla, para poder contar lo que hay sin traducir.
    ///
    /// LEER EL PROPIO CÓDIGO EN UN TEST parece raro y es lo único que funciona:
    /// la alternativa es acordarse de cuánto quedaba, y acordarse es justo lo
    /// que falla entre una sesión y la siguiente. Solo se compila en los tests.
    const FUENTE_ENTERA: &str = include_str!("main.rs");

    /// El fuente SIN los módulos de test.
    ///
    /// Los tests montan filas y paneles de mentira —«Etiqueta», «WIN-AD»,
    /// «Gemini 3.1 Pro — Esfuerzo Alto»— y contarlos como interfaz sin traducir
    /// inflaría la deuda con texto que nadie ve. Peor: haría que traducir de
    /// verdad no bajara el número, y un contador que no se mueve al trabajar se
    /// deja de mirar.
    fn fuente() -> &'static str {
        match FUENTE_ENTERA.find("#[cfg(test)]") {
            Some(i) => &FUENTE_ENTERA[..i],
            None => FUENTE_ENTERA,
        }
    }

    /// El primer literal que aparece después de cada `marca`.
    ///
    /// Un rascador a mano y no una expresión regular: meter el `regex` entero
    /// como dependencia para contar cadenas en un test sería pagar un compilado
    /// largo en cada `cargo test` por cuarenta líneas de trabajo.
    ///
    /// Entiende las continuaciones de línea de Rust (`\` al final), que se comen
    /// el salto Y la sangría siguiente SIN dejar espacio — si esto pusiera un
    /// espacio, los textos largos no casarían nunca con la tabla y el recuento
    /// diría que falta lo que ya está.
    /// Recorta a `n` bytes SIN partir un carácter.
    ///
    /// El fuente está lleno de comillas angulares y de rayas, que son de dos y
    /// tres bytes: cortar por un número redondo revienta a la primera. Se
    /// retrocede hasta el principio del carácter en el que caiga el corte.
    fn hasta(s: &str, n: usize) -> &str {
        let mut i = n.min(s.len());
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        &s[..i]
    }

    fn literales_tras(marca: &str, salto: usize) -> Vec<String> {
        let fuente = fuente();
        let bytes = fuente.as_bytes();
        let mut out = Vec::new();
        let mut desde = 0;
        while let Some(rel) = fuente[desde..].find(marca) {
            let ini = desde + rel + marca.len();
            desde = ini;
            // Se salta lo que haya antes del literal: en `panel(` son `ui`, la
            // columna y el icono, que no llevan comillas.
            let Some(mut i) = hasta(&fuente[ini..], salto).find('"') else {
                continue;
            };
            i += ini + 1;
            let mut s = String::new();
            while i < bytes.len() {
                match bytes[i] {
                    b'"' => break,
                    b'\\' => {
                        match bytes.get(i + 1) {
                            // Continuación: se come el salto y la sangría. El
                            // `\r` cuenta como salto — ver la nota de
                            // `literal_desde` sobre `core.autocrlf`.
                            Some(b'\n') | Some(b'\r') => {
                                i += 2;
                                while matches!(
                                    bytes.get(i),
                                    Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
                                ) {
                                    i += 1;
                                }
                                continue;
                            }
                            Some(b'n') => s.push('\n'),
                            Some(b'"') => s.push('"'),
                            Some(b'\\') => s.push('\\'),
                            // `\u{b7}` y demás: se descarta la cadena entera en
                            // vez de adivinar. Contar mal de menos es honesto;
                            // contar mal de más diría que hay cobertura que no
                            // hay.
                            _ => {
                                s.clear();
                                break;
                            }
                        }
                        i += 2;
                        continue;
                    }
                    _ => {
                        let c = fuente[i..].chars().next().unwrap();
                        s.push(c);
                        i += c.len_utf8();
                        continue;
                    }
                }
            }
            if s.len() >= 3 && s.chars().any(|c| c.is_alphabetic()) {
                out.push(s);
            }
        }
        out
    }

    #[test]
    fn ningun_sitio_pinta_texto_sin_pasarlo_por_la_traduccion() {
        // EL FALLO QUE TENÍA EL OTRO TEST, y que costó varias capturas del
        // operador descubrir: comprobaba que la frase estuviera EN LA TABLA, no
        // que el sitio que la pinta la pase por `tr`. Una frase traducida a
        // cinco idiomas cuyo `ui.button("↻ Recargar")` nunca se envolvió contaba
        // como cubierta y salía en español. Media aplicación estaba así mientras
        // el número decía cuarenta y ocho.
        //
        // Esto mide lo contrario: los SITIOS. Un literal con prosa dentro de una
        // llamada que pinta y sin `i18n::tr` cerca es una cadena que va a salir
        // en español pase lo que pase.
        //
        // Los ayudantes que traducen por dentro —`fila`, `panel`, `section`,
        // `insignia`, `segmentado`, `instrument_label`— no cuentan: sus
        // literales ya salen traducidos.
        const PINTAN: &[&str] = &[
            "RichText::new(",
            "ui.button(",
            "small_button(",
            ".on_hover_text(",
            ".hint_text(",
            "egui::Button::new(",
            ".selected_text(",
            // Añadidos DESPUÉS de que el visor de logs saliera en español con
            // este test en verde: son ayudantes propios de una pantalla, y cada
            // uno que aparezca hay que meterlo aquí a mano. Es la debilidad de
            // fondo de este test y no se arregla del todo — ver la nota del
            // tope.
            "seg(ui,",
            "lv_chip(ui,",
            "svc_row(ui,",
            "inv_tarjeta(ui,",
            // Las seis pestañas de Memoria salían en español porque esta marca
            // no estaba. Cada vez que aparece una pantalla con su propio
            // ayudante, la lista se queda corta hasta que alguien lo nota — y
            // hasta ahora lo ha notado siempre el operador, no este test.
            "selectable_label(",
        ];
        const TRADUCEN_SOLOS: &[&str] = &[
            "fila(",
            "panel(",
            "section(",
            "insignia(",
            "segmentado(",
            "instrument_label(",
            "cmp_tarjeta(",
            "etiqueta_campo(",
            "campo(ui,",
            // `di` es la única puerta por la que Lucy escribe una línea en el
            // hilo, así que traduce dentro. Cubre de un golpe todas las
            // respuestas de los comandos de barra que son un literal.
            "self.di(",
            "i18n::tr",
        ];
        let f = fuente();
        let lineas: Vec<&str> = f.lines().collect();
        let mut crudos: Vec<String> = Vec::new();
        for (i, l) in lineas.iter().enumerate() {
            let t = l.trim_start();
            if t.starts_with("//") {
                continue;
            }
            if !PINTAN.iter().any(|p| l.contains(p)) {
                continue;
            }
            // La ventana: el literal puede caer unas líneas por debajo de la
            // llamada, y el `tr` unas por encima.
            let desde = i.saturating_sub(3);
            let hasta = (i + 4).min(lineas.len());
            let ventana = lineas[desde..hasta].join("\n");
            if TRADUCEN_SOLOS.iter().any(|a| ventana.contains(a)) {
                continue;
            }
            // LAS PLANTILLAS TAMBIÉN CUENTAN, y antes se saltaban enteras con un
            // `continue`. Eso dejaba fuera de la medida todo un cubo: el
            // Dashboard salía en español —«32 núcleos», «570 GB libres de 931
            // GB», «act. 20:02»— con este test en verde, porque cada una de esas
            // frases vive dentro de un `format!`.
            //
            // Ahora hay `trf` para ellas, así que un `format!` en una llamada que
            // pinta y sin `trf` cerca es exactamente lo mismo que un literal
            // crudo: sale en español pase lo que pase.
            if ventana.contains("format!") && !ventana.contains("i18n::trf") {
                // Solo si el texto tiene PROSA fuera de los huecos: `{:.1} GB` es
                // un formato de cifra, no una frase, y no se traduce.
                let con_prosa = literales_de(l).into_iter().any(|s| {
                    let fuera: String =
                        s.split('{').map(|t| t.split_once('}').map_or(t, |(_, r)| r)).collect();
                    fuera.split_whitespace().filter(|p| p.chars().count() >= 3).count() >= 2
                });
                if con_prosa {
                    crudos.extend(literales_de(l).into_iter().filter(|s| s.contains('{')));
                }
                continue;
            }
            for s in literales_de(l) {
                // LAS SIGLAS CORTAS NO SE TRADUCEN. «CPU», «RAM», «PID» y
                // «OK» se escriben igual en los cinco idiomas; meterlas en la
                // tabla serian cuatro entradas con la misma palabra repetida
                // cinco veces, que es ruido con aspecto de trabajo.
                let sigla = s.chars().count() <= 4
                    && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
                if !sigla
                    && s.chars().count() >= 3
                    && s.chars().any(|c| c.is_alphabetic())
                    && !s.chars().all(|c| c.is_ascii_lowercase() || "._-/0123456789".contains(c))
                {
                    crudos.push(s);
                }
            }
        }
        crudos.sort();
        crudos.dedup();
        // CERO, Y SE QUEDA EN CERO. Cualquier sitio nuevo que pinte texto sin
        // pasarlo por `tr` o `trf` rompe este test en el commit que lo
        // introduce, que es la unica forma de que esto no se vuelva a pudrir.
        //
        // LO QUE ESTE TEST NO GARANTIZA, y hay que decirlo claro porque ha dado
        // cero cuatro veces mientras habia pantallas enteras en espanol:
        //
        //   1. La ventana es de tres lineas. Un `tr` vecino que no tiene nada
        //      que ver da por bueno el sitio de al lado, y un `format!` cuyo
        //      `ui.label` esta cinco lineas mas abajo no se ve.
        //   2. La lista de marcas de arriba es MANUAL. Cada ayudante nuevo de
        //      una pantalla hay que anadirlo, y hasta que alguien lo haga sus
        //      textos son invisibles para esto. Asi se escaparon Compliance y
        //      el visor de logs.
        //   3. Un literal guardado en una tabla de datos y pintado veinte
        //      lineas despues no lo ve ningun escaner de lineas.
        //
        // Es un CEDAZO, no una prueba. Caza el caso normal —una llamada nueva
        // escrita sin envolver— y ha cazado bastantes. Lo que no hace es
        // sustituir a abrir la aplicacion en otro idioma y mirarla.
        //
        // CERO, Y AQUI SE QUEDA. Este tope es distinto del de `la_cobertura`:
        // aquel mide DEUDA —lo que falta por traducir— y baja cuando se
        // traduce; este mide un ERROR —un sitio que pinta sin envolver— y no
        // hay ninguno que este bien. Subirlo no documenta nada: apaga el test.
        // SIN CONSTANTE `TOPE`, y por lo que dice el párrafo de arriba. Aquí
        // había un `const TOPE: usize = 0` con un `crudos.len() <= TOPE`, que es
        // una comparación que nunca puede ser falsa por otro motivo que la
        // igualdad — clippy la deniega, y llevaba desde el 19 de agosto haciendo
        // que `cargo clippy` de este binario terminara en error sin que nadie lo
        // notara. Como el tope es cero A PROPÓSITO y para siempre, la forma
        // honesta de escribirlo es «no puede haber ninguno»: la constante solo
        // dejaba abierta la puerta a subirla, que es justo lo que el comentario
        // dice que no hay que hacer.
        assert!(
            crudos.is_empty(),
            "{} sitios pintan texto sin pasarlo por la traducción, y no puede haber \
             ninguno. Estos salen en español en cualquier idioma:\n{}",
            crudos.len(),
            crudos
                .iter()
                .take(12)
                .map(|s| format!("  - {}", hasta(s, 60)))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// Los literales de una línea, ya desescapados de lo básico.
    /// El valor que tendrá un literal de Rust DESPUÉS de compilar.
    ///
    /// Lee desde la comilla de apertura, resuelve las continuaciones de línea y
    /// los escapes, y devuelve la cadena tal y como existirá en ejecución.
    ///
    /// HACE FALTA PORQUE LA TABLA GUARDA EL VALOR EN EJECUCIÓN. Comparar contra
    /// el texto del fuente da dos clases de mentira, y las dos costaron un rato:
    /// `"C:\\ruta"` leído del fuente nunca es igual al `C:\ruta` de la tabla,
    /// aunque en ejecución sean lo mismo —falso positivo, una frase traducida
    /// que se denuncia como pendiente—; y leer solo la primera línea de un
    /// literal partido con `\` deja media frase, que no está en la tabla ni lo
    /// estará nunca —falso positivo que no se puede arreglar traduciendo—.
    fn literal_desde(f: &str, ini: usize) -> Option<(String, usize)> {
        let b = f.as_bytes();
        if b.get(ini) != Some(&b'"') {
            return None;
        }
        let mut out = String::new();
        let mut i = ini + 1;
        while i < b.len() {
            match b[i] {
                b'"' => return Some((out, i + 1)),
                b'\\' => match b.get(i + 1) {
                    // Continuación: se come el salto y la sangría que sigue.
                    //
                    // `\r` CUENTA COMO SALTO, y no es un detalle de estilo. Este
                    // repositorio tiene `core.autocrlf=true`, así que en cuanto
                    // git toca el fichero las líneas pasan a CRLF y la barra de
                    // continuación deja de ir seguida de `\n`. Sin esta rama,
                    // TODOS los tests de traducción fallan en un clon recién
                    // hecho en Windows —que es la máquina de destino— y fallan
                    // de la peor manera: diciendo que veintidós frases no están
                    // traducidas cuando lo están.
                    Some(b'\n') | Some(b'\r') => {
                        i += 2;
                        if b.get(i) == Some(&b'\n') {
                            i += 1;
                        }
                        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
                            i += 1;
                        }
                    }
                    Some(b'n') => {
                        out.push('\n');
                        i += 2;
                    }
                    Some(b't') => {
                        out.push('\t');
                        i += 2;
                    }
                    Some(b'r') => {
                        out.push('\r');
                        i += 2;
                    }
                    Some(b'0') => {
                        out.push('\0');
                        i += 2;
                    }
                    Some(b'\\') => {
                        out.push('\\');
                        i += 2;
                    }
                    Some(b'"') => {
                        out.push('"');
                        i += 2;
                    }
                    Some(b'\'') => {
                        out.push('\'');
                        i += 2;
                    }
                    Some(b'u') if b.get(i + 2) == Some(&b'{') => {
                        let fin = f[i + 3..].find('}')? + i + 3;
                        out.push(char::from_u32(u32::from_str_radix(&f[i + 3..fin], 16).ok()?)?);
                        i = fin + 1;
                    }
                    _ => i += 2,
                },
                _ => {
                    let c = f[i..].chars().next()?;
                    out.push(c);
                    i += c.len_utf8();
                }
            }
        }
        None
    }

    /// El primer literal que sigue a `ini`, saltando espacios y saltos de línea.
    fn literal_tras_hueco(f: &str, ini: usize) -> Option<String> {
        let b = f.as_bytes();
        let mut j = ini;
        while j < b.len() && (b[j] == b' ' || b[j] == b'\n' || b[j] == b'\t' || b[j] == b'\r') {
            j += 1;
        }
        literal_desde(f, j).map(|(s, _)| s)
    }

    fn literales_de(l: &str) -> Vec<String> {
        let b = l.as_bytes();
        let (mut out, mut i) = (Vec::new(), 0);
        while i < b.len() {
            if b[i] != b'"' {
                i += 1;
                continue;
            }
            let ini = i + 1;
            let mut j = ini;
            while j < b.len() {
                match b[j] {
                    b'"' => break,
                    b'\\' => j += 2,
                    _ => j += 1,
                }
            }
            if j <= b.len() && ini <= j && l.is_char_boundary(ini) && l.is_char_boundary(j.min(l.len())) {
                out.push(l[ini..j.min(l.len())].to_string());
            }
            i = j + 1;
        }
        out
    }

    #[test]
    fn la_cobertura_no_puede_bajar() {
        // EL NÚMERO DE ABAJO SOLO PUEDE BAJAR. Es lo que convierte «¿cuánto
        // falta por traducir?» en algo que vigila el compilador en vez de algo
        // que hay que recordar entre una sesión y la siguiente — y es lo que
        // impide que la deuda vuelva a subir al añadir una pantalla nueva.
        //
        // Se cuentan los textos que PASAN POR UN AYUDANTE que traduce. Los que
        // no pasan por ahí no se traducen por mucho que estén en la tabla, y
        // contarlos daría una cobertura falsa.
        let mut todos: Vec<String> = Vec::new();
        // `fila(` y `panel(`: el primer literal es la etiqueta o el título. En
        // `panel(` hay tres argumentos sin comillas antes (ui, ancho, icono),
        // de ahí el salto mayor.
        todos.extend(literales_tras("fila(", 60));
        todos.extend(literales_tras("panel(", 140));
        todos.extend(literales_tras("insignia(ui,", 30));
        todos.extend(literales_tras("section(ui,", 30));
        todos.extend(literales_tras("i18n::tr(", 20));
        // Los subtítulos de `fila`, que son la mitad del texto de la pantalla y
        // que el primer literal de cada llamada no ve. `Some(` pilla también
        // algún `Some` que no es un subtítulo; da igual, son textos igualmente y
        // contarlos de más solo hace el listón más exigente.
        todos.extend(literales_tras("Some(", 2));
        // Y LAS FAMILIAS QUE NO TRADUCE NINGÚN AYUDANTE. Estas son el grueso de
        // la deuda: las etiquetas sueltas, los cuadros al pasar el ratón, los
        // botones y las pistas de los campos. Aquí no hay atajo — cada una hay
        // que envolverla en `tr` al traducir su pantalla.
        //
        // Las que YA están envueltas se detectan igual: el primer literal tras
        // `RichText::new(` sigue siendo el texto aunque haya un `i18n::tr(` en
        // medio, y como está en la tabla cuenta como cubierta.
        todos.extend(literales_tras("RichText::new(", 16));
        todos.extend(literales_tras(".on_hover_text(", 16));
        todos.extend(literales_tras("small_button(", 16));
        todos.extend(literales_tras(".hint_text(", 16));
        // LOS QUE FALTABAN, y se supo por una captura y no por este test: la
        // pantalla de Compliance salía entera en español con la interfaz en
        // alemán mientras el número de aquí decía que quedaban cincuenta y uno.
        // Un contador que no ve una pantalla entera da una falsa sensación de
        // avance, que es peor que no tener contador.
        todos.extend(literales_tras("ui.button(", 16));
        todos.extend(literales_tras("cmp_tarjeta(", 60));
        todos.extend(literales_tras("lv_chip(", 20));
        todos.extend(literales_tras("painter().text(", 120));
        todos.sort();
        todos.dedup();
        // Un literal VACÍO no es una frase sin traducir: es la rama muda de un
        // `if p.activo { "" } else { … }`. Contarlo hincha el número con algo
        // que nadie puede arreglar, y dos guiones en blanco en la lista de
        // fallos hacen dudar de la lista entera.
        todos.retain(|s| !s.trim().is_empty());

        let faltan: Vec<&String> = todos.iter().filter(|s| busca(s).is_none()).collect();

        // MEDIDO, NO ESTIMADO, y SOLO PUEDE BAJAR. Al traducir una pantalla este
        // número baja y se actualiza aquí; si sube, es que se ha añadido texto
        // sin pasarlo por la tabla — y entonces este test lo dice en el sitio y
        // en el momento en que ha pasado, en vez de aparecer meses después en
        // una captura con media pantalla en español.
        //
        // 95 → 51, y sigue en 51 tras traducir el Dashboard: al añadir `section`
        // a la lista de arriba afloraron tantos rótulos nuevos como frases se
        // habían traducido. No es un empate malo — es que el contador estaba
        // midiendo de menos y ahora mide más superficie por el mismo número.
        //
        // LO QUE ESTE TEST NO PUEDE VER, y conviene saberlo: solo mira ESTE
        // fichero. Los textos que llegan desde `lucy-core` —las etiquetas de
        // severidad y estado de compliance, los mensajes de error de los
        // módulos— quedan fuera de su vista, y se traducen envolviendo su salida
        // en el punto de uso. Ahí no hay red; hay que verlo en pantalla.
        //
        // 95 → 51 con Configuración y el resto de pantallas. Luego SUBIÓ a 57 al
        // ensanchar lo que mira, y bajó a 48 al traducir lo que apareció. Que
        // subiera es la parte buena: significa que dejó de mentir.
        //
        // 48 → 26, Y AQUÍ SE PARA. Las veintiséis que quedan no son deuda: son
        // lo que no se traduce nunca, y conviene saber cuáles para que nadie
        // gaste una tarde en ellas.
        //
        //   · La marca — «Lucy», «✦ Lucy», «Lucy v{}».
        //   · «prod, web, db», el ejemplo de la caja de etiquetas: traducirlo
        //     sugeriría etiquetar en otro idioma que el resto del equipo.
        //   · «local» y «motivo», que no son frases: una es el tipo de un
        //     equipo y la otra el NOMBRE de un hueco de `trf`.
        //   · Y el resto, veinte plantillas SIN UNA SOLA PALABRA dentro:
        //     `{n}`, `{pct:.0}%`, `→ {l}`, `{mb:.1} MB`, `▤ {etiqueta}`,
        //     `{os} · {ms} ms`. Lo único que se lee en ellas es el valor de la
        //     variable, que ya viene traducido de donde salga. Meterlas en la
        //     tabla sería escribir cinco veces la misma cadena.
        //
        // O sea: si esto vuelve a subir, es texto NUEVO sin traducir. No es una
        // de estas.
        //
        // 26 → 27 CON EL PANEL DE SALUD REMOTO (v2.1). Sube UNA, y sube siendo
        // del grupo de abajo: una plantilla de formato sin una sola palabra
        // dentro. Las SEIS frases con prosa que trajo ese panel —«Sondear»,
        // «Sondeando…», «Pulsa Sondear para pedirle su estado a este equipo»,
        // «El equipo no informó de ningún disco», «La sonda terminó sin
        // contestar» y «{h} h encendido»— están las seis en la tabla, en los
        // cinco idiomas. Se comprobó quitándolas de la lista una por una.
        //
        // NO SÉ CUÁL DE LAS PLANTILLAS ES, y lo digo en vez de inventarlo: el
        // listado que imprime este test no son literales del fuente sino texto
        // ya procesado por el rascador, así que no se puede casar buscando. Se
        // intentó por eliminación —convirtiendo a `trf` las cuatro plantillas
        // que el panel nuevo introduce— y el número no se movió, lo que apunta a
        // que la que cuenta la produce un ayudante y no una llamada directa.
        //
        // Que el tope suba por algo que no se puede nombrar es peor que si se
        // pudiera, y por eso queda escrito aquí: quien vuelva a tocar esto sabe
        // que hay una plantilla sin identificar dentro de la cuenta.
        const TOPE: usize = 27;
        assert!(
            faltan.len() <= TOPE,
            "{} textos sin traducir y el tope son {TOPE}. Si acabas de añadir \
             pantalla, tradúcela; si acabas de traducir una, baja el tope.\n\
             Los que quedan:\n{}",
            faltan.len(),
            faltan
                .iter()
                .map(|s| format!("  - {}", hasta(s, 70)))
                .collect::<Vec<_>>()
                .join("\n")
        );
        // Y que el recuento sirva de algo: si de pronto no encuentra textos, el
        // rascador se ha roto con algún cambio de formato y este test pasaría
        // siempre diciendo que todo está traducido.
        assert!(
            todos.len() > 60,
            "solo se han encontrado {} textos: el rascador ya no entiende el \
             código y este test ha dejado de medir nada",
            todos.len()
        );
    }

    #[test]
    fn las_etiquetas_de_una_memoria_se_leen_como_json_y_como_lista_vieja() {
        // La columna es un JSON: `["crystal","leccion"]`. Se leía quitando
        // corchetes y comillas con un `replace`, que funciona hasta que una
        // etiqueta lleve una coma dentro — y entonces se parte en dos etiquetas
        // que no existen, en silencio.
        assert_eq!(
            crate::mem_tags(r#"["crystal","leccion"]"#),
            vec!["crystal".to_string(), "leccion".to_string()]
        );
        // Una coma DENTRO de una etiqueta ya no la parte.
        assert_eq!(crate::mem_tags(r#"["dns, lento"]"#), vec!["dns, lento".to_string()]);
        // Y las filas viejas de la V1, escritas separadas por comas, no pierden
        // sus etiquetas por un cambio de formato: eso sería perder trabajo del
        // operador sin decírselo.
        assert_eq!(
            crate::mem_tags("prod, web"),
            vec!["prod".to_string(), "web".to_string()]
        );
        assert!(crate::mem_tags("").is_empty());
        assert!(crate::mem_tags("[]").is_empty());
    }

    #[test]
    fn las_plantillas_conservan_sus_huecos_en_los_cinco_idiomas() {
        // EL ÚNICO FALLO QUE ESTE MECANISMO PUEDE TENER. Si una traducción se
        // deja un `{n}`, la frase sale sin el número —«hace días»— y si se
        // inventa uno que no existe, sale el `{loquesea}` crudo en pantalla.
        // Las dos cosas pasan por descuido al traducir y ninguna la ve el
        // compilador, porque para él son cadenas.
        //
        // Se compara por CONJUNTO y no por orden: reordenar es justamente lo que
        // esto viene a permitir —«vor {n} Tagen» frente a «{n} days ago»— así
        // que exigir el mismo orden anularía el mecanismo entero.
        for f in FRASES {
            let esperados: std::collections::BTreeSet<&str> =
                huecos(f.es).into_iter().collect();
            if esperados.is_empty() {
                continue;
            }
            for (i, t) in f.otros.iter().enumerate() {
                let hay: std::collections::BTreeSet<&str> = huecos(t).into_iter().collect();
                assert_eq!(
                    hay,
                    esperados,
                    "«{}» en {}: los huecos no coinciden",
                    hasta(f.es, 40),
                    Lang::ALL[i + 1].nombre()
                );
            }
        }
    }

    #[test]
    fn los_huecos_se_leen_por_nombre_y_no_se_confunden() {
        assert_eq!(huecos("hace {n} días"), vec!["n"]);
        assert_eq!(huecos("{a} de {b} memorias"), vec!["a", "b"]);
        // Sin huecos, y sin confundir una llave suelta con uno.
        assert!(huecos("Configuración").is_empty());
        assert!(huecos("un { sin cerrar").is_empty());
        // Un `{}` posicional NO es un hueco con nombre: si alguien mete uno en la
        // tabla tiene que verse que no se va a rellenar, no colarse como válido.
        assert!(huecos("hace {} días").is_empty());
    }

    #[test]
    fn una_plantilla_se_rellena_en_el_idioma_puesto() {
        let _g = cerrojo();
        set(Lang::Es);
        assert_eq!(trf("hace {n} días", &[("n", "3")]), "hace 3 días");
        set(Lang::De);
        // Y en alemán el hueco va en otro sitio, que es el motivo de todo esto.
        assert_eq!(trf("hace {n} días", &[("n", "3")]), "vor 3 Tagen");
        set(Lang::Es);
    }

    #[test]
    fn la_marca_de_un_modelo_no_pasa_por_la_traduccion() {
        // «Gemini 3.5 Blitz» en el selector sería un fallo que nadie sabría
        // explicar. La mitad izquierda identifica el modelo y no la toca nadie;
        // solo se traduce lo que va detrás de la raya.
        let _g = cerrojo();
        set(Lang::De);
        let s = modelo("Gemini 3.5 Flash — Rendimiento de frontera sostenido");
        assert!(s.starts_with("Gemini 3.5 Flash — "), "la marca se ha tocado: {s}");
        assert!(!s.contains("Rendimiento"), "la descripción no se ha traducido: {s}");
        // Sin raya —los ids de Ollama— se devuelve tal cual.
        assert_eq!(modelo("mistral:latest"), "mistral:latest");
        set(Lang::Es);
    }

    #[test]
    fn todas_las_descripciones_del_catalogo_estan_traducidas() {
        // Recorriendo el catálogo DE VERDAD: un modelo nuevo con descripción
        // sin traducir rompe este test en vez de aparecer en una captura.
        let mut faltan: Vec<&str> = Vec::new();
        for g in lucy_core::models::GROUPS {
            for o in g.options {
                if let Some((_, desc)) = o.name.split_once(" — ") {
                    // LOS COMANDOS NO SE TRADUCEN. La entrada de Ollama lleva
                    // `ollama pull <model>` como «descripción» porque es lo que
                    // hay que teclear para instalarlo: traducirlo daría una
                    // orden que no existe, y quien la copie se encontrará un
                    // «comando no reconocido» sin entender por qué.
                    if desc.starts_with("ollama ") {
                        continue;
                    }
                    if busca(desc).is_none() {
                        faltan.push(desc);
                    }
                }
            }
        }
        faltan.sort();
        faltan.dedup();
        assert!(
            faltan.is_empty(),
            "{} descripciones del catálogo de modelos salen en español:\n{}",
            faltan.len(),
            faltan.iter().take(8).map(|s| format!("  - {s}")).collect::<Vec<_>>().join("\n")
        );
    }

    #[test]
    fn toda_frase_envuelta_en_tr_tiene_traduccion() {
        // EL HUECO QUE FALTABA, y por el que se colaron los vacíos de Inventario
        // y del visor. Había un test que comprobaba que los SITIOS estuvieran
        // envueltos y otro que las frases DE LA TABLA estuvieran completas — y
        // ninguno miraba lo de en medio: una frase envuelta en `tr` que no está
        // en la tabla.
        //
        // `tr` devuelve el español cuando no encuentra la frase, y eso es a
        // propósito: permite convertir por pantallas sin que las que faltan se
        // vean rotas. Pero significa que envolver una cadena y olvidar
        // traducirla no falla en ninguna parte — sale en español y el sitio
        // cuenta como cubierto.
        let f = fuente();
        let mut faltan: Vec<String> = Vec::new();
        // LAS PLANTILLAS DE `trf` CUENTAN IGUAL, y antes no se miraban: `tr(` no
        // encuentra `trf(` porque tras «tr» viene una efe y no un paréntesis.
        // Media pantalla de Configuración es `trf` y estaba fuera del test.
        for marca in ["i18n::tr(", "i18n::trf("] {
            let mut desde = 0;
            while let Some(rel) = f[desde..].find(marca) {
                let ini = desde + rel + marca.len();
                desde = ini;
                // Un `tr(msg)` con una variable dentro no se puede resolver
                // leyendo el fuente, y ahí no hay nada que comprobar.
                let Some(s) = literal_tras_hueco(&f, ini) else { continue };
                // Sin prosa, o una sigla: no se traducen.
                let sigla = s.chars().count() <= 4
                    && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
                if sigla || s.chars().count() < 3 || !s.chars().any(|c| c.is_alphabetic()) {
                    continue;
                }
                if busca(&s).is_none() {
                    faltan.push(s);
                }
            }
        }
        faltan.sort();
        faltan.dedup();
        // TRES, Y SON LAS TRES A PROPOSITO: «Lucy» y «✦ Lucy» son la marca, y
        // «prod, web, db» es el ejemplo de la caja de etiquetas — traducirlo
        // sugeriria etiquetar en otro idioma que el resto del equipo.
        //
        // Van envueltas igualmente porque pasan por un ayudante que traduce, y
        // sacarlas de ahi seria complicar el sitio de llamada para complacer a
        // un test. `tr` devuelve el español y eso es lo correcto para las tres.
        const TOPE: usize = 3;
        assert!(
            faltan.len() <= TOPE,
            "{} frases pasan por `tr` y NO están en la tabla, así que salen en \
             español en cualquier idioma:\n{}",
            faltan.len(),
            faltan.iter().map(|s| format!("  - {}", hasta(s, 90)))
                .collect::<Vec<_>>().join("\n")
        );
    }

    #[test]
    fn los_textos_que_vienen_del_core_estan_traducidos() {
        // EL PUNTO CIEGO QUE NO CERRABA NINGÚN OTRO TEST. `lucy-core` no sabe de
        // idiomas —y no debe: es el dominio, y la app Tauri comparte el mismo
        // crate— así que sus `label()` se traducen envolviéndolos en el punto de
        // uso. Eso funciona, pero nada avisaba de que una variante NUEVA en el
        // core llegara a la pantalla sin traducción: se descubría en una
        // captura, que es como se han descubierto casi todos los de esta tanda.
        //
        // Recorriendo los enums de verdad, añadir una sexta categoría de
        // inventario o un cuarto tono rompe este test hasta que alguien la
        // traduzca. Los `label()` que son nombres propios —los protocolos,
        // «Windows (WinRM)»— quedan fuera a propósito.
        let mut faltan: Vec<&str> = Vec::new();
        for c in lucy_core::inventory::Categoria::ALL {
            if busca(c.label()).is_none() {
                faltan.push(c.label());
            }
        }
        for t in lucy_core::prompt::Tono::ALL {
            if busca(t.label()).is_none() {
                faltan.push(t.label());
            }
        }
        for e in [
            lucy_core::compliance::Estado::Pasa,
            lucy_core::compliance::Estado::Aviso,
            lucy_core::compliance::Estado::Falla,
            lucy_core::compliance::Estado::Error,
        ] {
            // «OK» es igual en los cinco idiomas y no está en la tabla a
            // propósito: `tr` devuelve el español, que es lo que se quiere.
            if e.label() != "OK" && busca(e.label()).is_none() {
                faltan.push(e.label());
            }
        }
        // Los campos que `Host::missing()` nombra —«nombre», «dirección»,
        // «usuario»— salen en la franja ámbar del formulario de equipo remoto, y
        // vienen del core como los demás.
        for c in ["nombre", "dirección", "usuario"] {
            if busca(c).is_none() {
                faltan.push(c);
            }
        }
        // Y el requisito de cada protocolo, que se enseña ANTES de intentar la
        // conexión. Solo los que dicen algo: la mayoría lo tienen vacío.
        for p in lucy_core::hosts::Protocol::ALL {
            let r = p.requirement();
            if !r.is_empty() && busca(r).is_none() {
                faltan.push(r);
            }
        }
        assert!(
            faltan.is_empty(),
            "estos textos vienen de `lucy-core` y llegan a la pantalla sin \
             traducción:\n{}",
            faltan.iter().map(|s| format!("  - {s}")).collect::<Vec<_>>().join("\n")
        );
    }

    #[test]
    fn el_orden_de_los_idiomas_casa_con_el_de_los_textos() {
        // Si alguien reordena `Lang::ALL` sin tocar las tablas, cada idioma
        // enseñaría el texto de otro — y en cuatro columnas de texto plano eso
        // no se ve leyendo el diff.
        for (i, l) in Lang::ALL.into_iter().enumerate() {
            assert_eq!(l.idx(), i, "{} no está en su sitio", l.nombre());
        }
        assert_eq!(Lang::Es.idx(), 0, "el español tiene que ser el primero");
    }
}
