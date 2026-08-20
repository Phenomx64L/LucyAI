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
        "(sin cambios)",
        "(no changes)",
        "(sem alterações)",
        "(aucun changement)",
        "(keine Änderungen)",
    ),
    f!(
        "1 comando propuesto — apruébalo en el panel de Plan",
        "1 command proposed — approve it in the Plan panel",
        "1 comando proposto — aprova-o no painel de Plano",
        "1 commande proposée — approuve-la dans le panneau Plan",
        "1 Befehl vorgeschlagen — gib ihn im Plan-Panel frei",
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
    f!(
        "Aplicar el cambio en {ruta}",
        "Apply the change to {ruta}",
        "Aplicar a alteração em {ruta}",
        "Appliquer la modification dans {ruta}",
        "Änderung in {ruta} übernehmen",
    ),
    f!("Aprobar", "Approve", "Aprovar", "Approuver", "Genehmigen"),
    f!("Archivo", "Archive", "Arquivo", "Fichiers", "Archiv"),
    f!("Artefactos", "Artifacts", "Artefactos", "Artefacts", "Artefakte"),
    f!("Atención", "Warning", "Atenção", "Attention", "Achtung"),
    f!("Auditoría", "Audit", "Auditoria", "Audit", "Audit"),
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
    f!("CONFORMES", "COMPLIANT", "CONFORMES", "CONFORMES", "ERFÜLLT"),
    f!("CPU alta ({pct}%)", "High CPU ({pct}%)", "CPU alta ({pct}%)", "CPU élevé ({pct}%)", "Hohe CPU-Last ({pct}%)"),
    f!(
        "Cada cristal es una sesión destilada. Se escriben solos al cerrar turnos; sus lecciones ya son memorias y sobreviven aunque borres el cristal.",
        "Each crystal is a distilled session. They write themselves when turns close; their lessons are already memories and survive even if you delete the crystal.",
        "Cada cristal é uma sessão destilada. Escrevem-se sozinhos ao fechar turnos; as suas lições já são memórias e sobrevivem mesmo que apagues o cristal.",
        "Chaque cristal est une session distillée. Ils s'écrivent seuls à la fin des tours ; leurs leçons sont déjà des mémoires et survivent même si tu supprimes le cristal.",
        "Jeder Kristall ist eine destillierte Sitzung. Sie schreiben sich selbst, wenn Runden enden; ihre Lehren sind bereits Erinnerungen und bleiben, auch wenn du den Kristall löschst.",
    ),
    f!("Cancelar", "Cancel", "Cancelar", "Annuler", "Abbrechen"),
    f!("Cargando…", "Loading…", "A carregar…", "Chargement…", "Lädt…"),
    f!(
        "Cargas sensibles al costo",
        "Cost-sensitive workloads",
        "Cargas sensíveis ao custo",
        "Charges sensibles au coût",
        "Kostensensible Workloads",
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
    f!("Dirección", "Address", "Endereço", "Adresse", "Adresse"),
    f!("Disco sistema", "System disk", "Disco do sistema", "Disque système", "Systemlaufwerk"),
    f!("Discos", "Disks", "Discos", "Disques", "Datenträger"),
    f!(
        "Dispositivo de red",
        "Network device",
        "Dispositivo de rede",
        "Équipement réseau",
        "Netzwerkgerät",
    ),
    f!("Documentos", "Documents", "Documentos", "Documents", "Dokumente"),
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
        "El fichero no tiene líneas.",
        "The file has no lines.",
        "O ficheiro não tem linhas.",
        "Le fichier n'a aucune ligne.",
        "Die Datei hat keine Zeilen.",
    ),
    f!("El más barato", "Cheapest", "O mais barato", "Le moins cher", "Am günstigsten"),
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
        "Elige la carpeta de un skill, o una que contenga varios — un repositorio descargado sirve tal cual",
        "Pick a skill's folder, or one holding several — a downloaded repository works as is",
        "Escolhe a pasta de um skill, ou uma que contenha vários — um repositório descarregado serve tal como está",
        "Choisis le dossier d'un skill, ou un dossier qui en contient plusieurs — un dépôt téléchargé convient tel quel",
        "Wähl den Ordner eines Skills, oder einen mit mehreren — ein heruntergeladenes Repository funktioniert direkt",
    ),
    f!("Eliminar", "Delete", "Eliminar", "Supprimer", "Löschen"),
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
    f!("Equipos", "Machines", "Máquinas", "Machines", "Rechner"),
    f!(
        "Error de conexión",
        "Connection error",
        "Erro de ligação",
        "Erreur de connexion",
        "Verbindungsfehler",
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
        "Escribe una orden y Lucy la ejecuta — el plan, la salida y el trace\n\
         se llenan en el workspace →",
        "Type a command and Lucy runs it — the plan, the output and the trace\n\
         fill up in the workspace →",
        "Escreve uma ordem e a Lucy executa-a — o plano, a saída e o rasto\n\
         preenchem-se no workspace →",
        "Écris une commande et Lucy l'exécute — le plan, la sortie et la trace\n\
         se remplissent dans le workspace →",
        "Schreib einen Befehl und Lucy führt ihn aus — Plan, Ausgabe und Spur\n\
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
    f!("Este equipo", "This machine", "Esta máquina", "Cette machine", "Dieser Rechner"),
    f!(
        "Este modelo no tiene precio en el catálogo",
        "This model has no price in the catalog",
        "Este modelo não tem preço no catálogo",
        "Ce modèle n'a pas de prix au catalogue",
        "Für dieses Modell gibt es keinen Preis im Katalog",
    ),
    f!("Etiquetas", "Tags", "Etiquetas", "Étiquettes", "Tags"),
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
        "Frontera (trabajo profesional complejo)",
        "Frontier (complex professional work)",
        "Fronteira (trabalho profissional complexo)",
        "Frontière (travail professionnel complexe)",
        "Frontier (komplexe Profi-Arbeit)",
    ),
    f!("Fundir", "Merge", "Fundir", "Fusionner", "Zusammenführen"),
    f!(
        "Google vía NVIDIA",
        "Google via NVIDIA",
        "Google via NVIDIA",
        "Google via NVIDIA",
        "Google über NVIDIA",
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
        "La salida de cada comando aparece aquí en vivo mientras el agente trabaja.",
        "Each command's output appears here live while the agent works.",
        "A saída de cada comando aparece aqui em direto enquanto o agente trabalha.",
        "La sortie de chaque commande s'affiche ici en direct pendant que l'agent travaille.",
        "Die Ausgabe jedes Befehls erscheint hier live, während der Agent arbeitet.",
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
        "Lucy todavía no ha apuntado nada sobre ti. Lo hace sola cuando le cuentas algo que le servirá otro día.",
        "Lucy hasn't noted anything about you yet. She does it on her own when you tell her something that will help another day.",
        "A Lucy ainda não apontou nada sobre ti. Fá-lo sozinha quando lhe contas algo que lhe servirá noutro dia.",
        "Lucy n'a encore rien noté sur toi. Elle le fait seule quand tu lui dis quelque chose qui lui servira un autre jour.",
        "Lucy hat noch nichts über dich notiert. Sie macht das von selbst, wenn du ihr etwas erzählst, das ihr an einem anderen Tag nützt.",
    ),
    f!(
        "Línea base: {etiqueta} · {cuando}",
        "Baseline: {etiqueta} · {cuando}",
        "Linha base: {etiqueta} · {cuando}",
        "Ligne de base : {etiqueta} · {cuando}",
        "Baseline: {etiqueta} · {cuando}",
    ),
    f!("Mantenimiento", "Maintenance", "Manutenção", "Maintenance", "Wartung"),
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
    f!("Minimizar", "Minimize", "Minimizar", "Réduire", "Minimieren"),
    f!("Modelo activo", "Active model", "Modelo ativo", "Modèle actif", "Aktives Modell"),
    f!(
        "Modelo y comportamiento",
        "Model and behaviour",
        "Modelo e comportamento",
        "Modèle et comportement",
        "Modell und Verhalten",
    ),
    f!("Modo privacidad", "Privacy mode", "Modo privacidade", "Mode confidentialité", "Privatmodus"),
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
        "No se pudo leer «{ruta}»: {e}",
        "Could not read «{ruta}»: {e}",
        "Não foi possível ler «{ruta}»: {e}",
        "Impossible de lire « {ruta} » : {e}",
        "«{ruta}» konnte nicht gelesen werden: {e}",
    ),
    f!(
        "No se pudo traducir: {e}",
        "Could not translate: {e}",
        "Não foi possível traduzir: {e}",
        "Impossible de traduire : {e}",
        "Übersetzung nicht möglich: {e}",
    ),
    f!("Nombre", "Name", "Nome", "Nom", "Name"),
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
        "Pide el catálogo de modelos — no gasta",
        "Fetches the model catalog — costs nothing",
        "Pede o catálogo de modelos — não gasta",
        "Demande le catalogue de modèles — ne coûte rien",
        "Fragt den Modellkatalog ab — kostet nichts",
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
    f!("Protocolo", "Protocol", "Protocolo", "Protocole", "Protokoll"),
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
    f!("RAM alta ({pct}%)", "High RAM ({pct}%)", "RAM alta ({pct}%)", "RAM élevée ({pct}%)", "Hohe RAM-Auslastung ({pct}%)"),
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
    f!("Red", "Network", "Rede", "Réseau", "Netzwerk"),
    f!("Rehacer", "Redo", "Refazer", "Rétablir", "Wiederholen"),
    f!("Rehaciendo…", "Rebuilding…", "A refazer…", "Reconstruction…", "Neuaufbau…"),
    f!("Reintentar", "Retry", "Tentar de novo", "Réessayer", "Wiederholen"),
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
    f!("Saludable", "Healthy", "Saudável", "Sain", "Gesund"),
    f!(
        "Se guardan en el Credential Manager de Windows, en el mismo sitio del que las lee la app de escritorio. Ollama no necesita clave: es local.",
        "They're saved in the Windows Credential Manager, the same place the desktop app reads them from. Ollama needs no key: it's local.",
        "Guardam-se no Credential Manager do Windows, no mesmo sítio de onde a app de desktop as lê. O Ollama não precisa de chave: é local.",
        "Elles sont enregistrées dans le Credential Manager de Windows, là où l'appli de bureau les lit. Ollama n'a pas besoin de clé : il est local.",
        "Sie werden im Credential Manager von Windows gespeichert, dort, wo die Desktop-App sie liest. Ollama braucht keinen Schlüssel: läuft lokal.",
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
    f!("Sin resultados", "No results", "Sem resultados", "Aucun résultat", "Keine Treffer"),
    f!("Sistema", "System", "Sistema", "Système", "System"),
    f!("Skills", "Skills", "Skills", "Skills", "Skills"),
    f!("Software", "Software", "Software", "Logiciels", "Software"),
    f!("Sub-agentes", "Sub-agents", "Subagentes", "Sous-agents", "Sub-Agenten"),
    f!("Tareas", "Tasks", "Tarefas", "Tâches", "Aufgaben"),
    f!("Tema", "Theme", "Tema", "Thème", "Design"),
    f!("Terminales", "Terminals", "Terminais", "Terminaux", "Terminals"),
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
        "Tope de gasto de la sesión",
        "Session spending cap",
        "Limite de gasto da sessão",
        "Plafond de dépense de la session",
        "Ausgabenlimit der Sitzung",
    ),
    f!(
        "Tope de pasos seguidos",
        "Consecutive steps cap",
        "Limite de passos seguidos",
        "Plafond d'étapes enchaînées",
        "Limit für Schritte in Folge",
    ),
    f!("Trace vacío", "Empty trace", "Trace vazio", "Trace vide", "Trace leer"),
    f!("Transcribiendo…", "Transcribing…", "A transcrever…", "Transcription…", "Transkribiere…"),
    f!(
        "Trozos sin vector",
        "Chunks with no vector",
        "Fragmentos sem vetor",
        "Fragments sans vecteur",
        "Textstücke ohne Vektor",
    ),
    f!("Tu nombre", "Your name", "O teu nome", "Ton nom", "Dein Name"),
    f!(
        "Un comando, o pídemelo en español…   ·   ↑↓ historial",
        "A command, or just ask me in plain English…   ·   ↑↓ history",
        "Um comando, ou pede-mo em português…   ·   ↑↓ histórico",
        "Une commande, ou demande-le-moi en français…   ·   ↑↓ historique",
        "Ein Befehl, oder frag mich auf Deutsch…   ·   ↑↓ Verlauf",
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
    f!("Usuario", "Username", "Utilizador", "Utilisateur", "Benutzer"),
    f!(
        "Ver cambios",
        "View changes",
        "Ver alterações",
        "Voir les changements",
        "Änderungen ansehen",
    ),
    f!("Ver detalle", "Show details", "Ver detalhe", "Voir le détail", "Details anzeigen"),
    f!(
        "Ver la evidencia",
        "View the evidence",
        "Ver as provas",
        "Voir la preuve",
        "Nachweis ansehen",
    ),
    f!("Ver sus logs", "View its logs", "Ver os seus logs", "Voir ses logs", "Logs ansehen"),
    f!("Visor de logs", "Log viewer", "Visor de logs", "Visionneuse de logs", "Log-Ansicht"),
    // ── La ayuda de cada módulo ──────────────────────────────────────────────
    f!(
        "Volver al inventario",
        "Back to inventory",
        "Voltar ao inventário",
        "Retour à l'inventaire",
        "Zurück zum Inventar",
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
    f!("act. {hora}", "upd. {hora}", "atu. {hora}", "maj {hora}", "akt. {hora}"),
    f!("ahora", "now", "agora", "à l'instant", "jetzt"),
    f!("alta", "high", "alta", "élevée", "hoch"),
    f!(
        "antes de mandar una tarea exigente · no cambia el modelo por ti",
        "before you send a demanding task · doesn't switch the model for you",
        "antes de enviar uma tarefa exigente · não muda o modelo por ti",
        "avant d'envoyer une tâche exigeante · ne change pas le modèle à ta place",
        "vor einer anspruchsvollen Aufgabe · wechselt das Modell nicht für dich",
    ),
    f!("audit trail", "audit trail", "registo de auditoria", "piste d'audit", "Audit-Trail"),
    f!("baja", "low", "baixa", "faible", "niedrig"),
    f!(
        "buscable por significado",
        "searchable by meaning",
        "pesquisável por significado",
        "recherchable par le sens",
        "nach Bedeutung durchsuchbar",
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
    f!(
        "consistente, aunque Lucy esté escribiendo",
        "consistent, even while Lucy is typing",
        "consistente, mesmo com a Lucy a escrever",
        "cohérent, même si Lucy écrit",
        "konsistent, auch während Lucy schreibt",
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
    f!("dirección", "address", "endereço", "adresse", "Adresse"),
    f!("editado", "edited", "editado", "édité", "bearbeitet"),
    f!(
        "en producción avisa antes de reiniciar un servicio",
        "in production, warn before restarting a service",
        "em produção avisa antes de reiniciar um serviço",
        "en production, prévient avant de redémarrer un service",
        "in der Produktion warnt sie vor dem Neustart eines Dienstes",
    ),
    f!("en vivo · {hora}", "live · {hora}", "ao vivo · {hora}", "en direct · {hora}", "live · {hora}"),
    f!("escaneando… {s}s", "scanning… {s}s", "a analisar… {s}s", "analyse… {s}s", "scanne… {s}s"),
    f!(
        "escribe owner/model",
        "type owner/model",
        "escreve owner/model",
        "écris owner/model",
        "gib owner/model ein",
    ),
    f!("escrito", "written", "escrito", "écrit", "geschrieben"),
    f!(
        "escritura progresiva y transiciones · LUCY_NO_MOTION=1 las apaga al arrancar",
        "progressive typing and transitions · LUCY_NO_MOTION=1 turns them off at startup",
        "escrita progressiva e transições · LUCY_NO_MOTION=1 desliga-as ao arrancar",
        "écriture progressive et transitions · LUCY_NO_MOTION=1 les désactive au démarrage",
        "schrittweise Ausgabe und Übergänge · LUCY_NO_MOTION=1 schaltet sie beim Start aus",
    ),
    f!(
        "filtrar por texto — Intro para búsqueda semántica",
        "filter by text — Enter for semantic search",
        "filtrar por texto — Enter para pesquisa semântica",
        "filtrer par texte — Entrée pour la recherche sémantique",
        "nach Text filtern — Enter für semantische Suche",
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
        "lo que se ilumina: navegación, progreso, hecho",
        "what lights up: navigation, progress, done",
        "o que se ilumina: navegação, progresso, concluído",
        "ce qui s'allume : navigation, progression, terminé",
        "was hervorgehoben wird: Navigation, Fortschritt, erledigt",
    ),
    f!("media", "medium", "média", "moyenne", "mittel"),
    f!("memoria", "memory", "memória", "mémoire", "Gedächtnis"),
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
    f!(
        "si se deja vacío usa el usuario de Windows, que es una cuenta y no un nombre",
        "if left empty it uses the Windows user, which is an account and not a name",
        "se ficar vazio usa o utilizador do Windows, que é uma conta e não um nome",
        "si tu le laisses vide, on prend l'utilisateur Windows, qui est un compte et pas un nom",
        "wenn leer, gilt der Windows-Benutzer, und das ist ein Konto, kein Name",
    ),
    f!("sin clave", "no key", "sem chave", "sans clé", "ohne Schlüssel"),
    f!(
        "sin dirección aún",
        "no address yet",
        "sem endereço ainda",
        "pas encore d’adresse",
        "noch keine Adresse",
    ),
    f!("sin saber", "unknown", "por saber", "inconnue", "ungeprüft"),
    f!(
        "solo se encuentran por palabras — pasó si Ollama estaba caído al ingerir",
        "only found by words — happened if Ollama was down at ingest",
        "só se encontram por palavras — aconteceu se o Ollama estava em baixo ao ingerir",
        "on ne les trouve que par mots — arrive si Ollama était en panne à l'ingestion",
        "nur über Wörter auffindbar — passiert, wenn Ollama beim Einlesen aus war",
    ),
    f!(
        "todo el tráfico a Ollama local",
        "all traffic to local Ollama",
        "todo o tráfego para o Ollama local",
        "tout le trafic vers Ollama local",
        "aller Datenverkehr zum lokalen Ollama",
    ),
    f!("traduciendo…", "translating…", "a traduzir…", "traduction…", "übersetze…"),
    f!("usuario", "username", "utilizador", "utilisateur", "Benutzer"),
    f!(
        "vacío = ssh-agent o ~/.ssh/id_ed25519",
        "empty = ssh-agent or ~/.ssh/id_ed25519",
        "vazio = ssh-agent ou ~/.ssh/id_ed25519",
        "vide = ssh-agent ou ~/.ssh/id_ed25519",
        "leer = ssh-agent oder ~/.ssh/id_ed25519",
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
        "{ent} tokens de entrada, {sal} de salida en esta terminal",
        "{ent} input tokens, {sal} output in this terminal",
        "{ent} tokens de entrada, {sal} de saída neste terminal",
        "{ent} tokens en entrée, {sal} en sortie dans ce terminal",
        "{ent} Tokens Eingabe, {sal} Ausgabe in diesem Terminal",
    ),
    f!(
        "{grupos} grupos · {memorias} memorias se fundirían en otra, de {miradas} miradas. No se ha tocado nada todavía.",
        "{grupos} groups · {memorias} memories would merge into another, out of {miradas} looked at. Nothing has been touched yet.",
        "{grupos} grupos · {memorias} memórias seriam fundidas noutra, de {miradas} vistas. Ainda não se tocou em nada.",
        "{grupos} groupes · {memorias} mémoires fusionneraient dans une autre, sur {miradas} examinées. Rien n'a encore été modifié.",
        "{grupos} Gruppen · {memorias} Erinnerungen würden zu einer verschmelzen, aus {miradas} Sichtungen. Es wurde noch nichts verändert.",
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
        "{n} ficheros en {dir} — el más reciente primero",
        "{n} files in {dir} — newest first",
        "{n} ficheiros em {dir} — o mais recente primeiro",
        "{n} fichiers dans {dir} — le plus récent en premier",
        "{n} Dateien in {dir} — die neueste zuerst",
    ),
    f!(
        "{n} memorias detrás",
        "{n} memories behind it",
        "{n} memórias por trás",
        "{n} mémoires derrière",
        "{n} Erinnerungen dahinter",
    ),
    f!("{n} núcleos", "{n} cores", "{n} núcleos", "{n} cœurs", "{n} Kerne"),
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
    f!("{n} volúmenes", "{n} volumes", "{n} volumes", "{n} volumes", "{n} Laufwerke"),
    f!(
        "{pista} · el proveedor la acepta",
        "{pista} · the provider accepts it",
        "{pista} · o fornecedor aceita-a",
        "{pista} · le fournisseur l'accepte",
        "{pista} · der Anbieter akzeptiert ihn",
    ),
    f!(
        "{vivas} de {total} memorias vivas",
        "{vivas} of {total} live memories",
        "{vivas} de {total} memórias vivas",
        "{vivas} sur {total} mémoires vivantes",
        "{vivas} von {total} lebenden Erinnerungen",
    ),
    f!(
        "· {n} dinámicos ignorados",
        "· {n} dynamic ones ignored",
        "· {n} dinâmicos ignorados",
        "· {n} dynamiques ignorés",
        "· {n} dynamische ignoriert",
    ),
    f!("¿borrar?", "delete?", "apagar?", "supprimer ?", "löschen?"),
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
    f!("◈ Semántica", "◈ Semantic", "◈ Semântica", "◈ Sémantique", "◈ Semantisch"),
    f!("● ESCANEADO {hora}", "● SCANNED {hora}", "● ANALISADO {hora}", "● ANALYSÉ {hora}", "● GESCANNT {hora}"),
    f!(
        "⚠ {cat}: se enseñan {vistas} de {total}. Una lista recortada en silencio se lee como una lista completa.",
        "⚠ {cat}: showing {vistas} of {total}. A list trimmed in silence reads like a complete one.",
        "⚠ {cat}: mostram-se {vistas} de {total}. Uma lista cortada em silêncio lê-se como uma lista completa.",
        "⚠ {cat} : affichage de {vistas} sur {total}. Une liste tronquée en silence se lit comme une liste complète.",
        "⚠ {cat}: {vistas} von {total} werden angezeigt. Eine still gekürzte Liste liest sich wie eine vollständige.",
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
    fn la_tabla_esta_ordenada_porque_la_busqueda_es_binaria() {
        // Una frase fuera de orden no falla: hace que ESA no se encuentre nunca
        // y salga en español para siempre, sin que nada lo diga.
        for par in FRASES.windows(2) {
            assert!(
                par[0].es < par[1].es,
                "«{}» va después de «{}» y la tabla tiene que ir ordenada",
                &par[0].es[..par[0].es.len().min(40)],
                &par[1].es[..par[1].es.len().min(40)]
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
                            // Continuación: se come el salto y la sangría.
                            Some(b'\n') => {
                                i += 2;
                                while matches!(bytes.get(i), Some(b' ') | Some(b'\r')) {
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
        const TOPE: usize = 0;
        assert!(
            crudos.len() <= TOPE,
            "{} sitios pintan texto sin pasarlo por la traducción y el tope son \
             {TOPE}. Estos salen en español en cualquier idioma:\n{}",
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
        // Lo que queda son casi todas PLANTILLAS CON HUECO
        // —`Falta: {}`, `Listo para operar en {}`— y esas no se arreglan
        // metiendo la cadena en la tabla: el orden de los huecos cambia entre
        // idiomas y hay que decidir caso por caso si se traduce la plantilla o
        // se compone la frase de otra forma. Más los nombres de marca, que no se
        // traducen y nunca bajarán de aquí.
        // LO QUE ESTE TEST NO PUEDE VER, y conviene saberlo: solo mira ESTE
        // fichero. Los textos que llegan desde `lucy-core` —las etiquetas de
        // severidad y estado de compliance, los mensajes de error de los
        // módulos— quedan fuera de su vista, y se traducen envolviendo su salida
        // en el punto de uso. Ahí no hay red; hay que verlo en pantalla.
        // 95 → 51 con Configuración y el resto de pantallas. Luego SUBIÓ a 57 al
        // ensanchar lo que mira, y bajó a 48 al traducir lo que aparecio. Que
        // subiera es la parte buena: significa que dejó de mentir.
        const TOPE: usize = 48;
        assert!(
            faltan.len() <= TOPE,
            "{} textos sin traducir y el tope son {TOPE}. Si acabas de añadir \
             pantalla, tradúcela; si acabas de traducir una, baja el tope.\n\
             Los diez primeros:\n{}",
            faltan.len(),
            faltan
                .iter()
                .take(10)
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
        let mut desde = 0;
        while let Some(rel) = f[desde..].find("i18n::tr(") {
            let ini = desde + rel + "i18n::tr(".len();
            desde = ini;
            // Solo si el literal va PEGADO a la llamada. Un `tr(msg)` con una
            // variable dentro no se puede resolver leyendo el fuente, y ahí no
            // hay nada que comprobar desde aquí.
            let resto = hasta(&f[ini..], 400);
            let Some(c) = resto.chars().next() else { continue };
            if c != '"' {
                continue;
            }
            let Some(s) = literales_de(resto.lines().next().unwrap_or("")).into_iter().next()
            else {
                continue;
            };
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
            faltan.iter().take(12).map(|s| format!("  - {}", hasta(s, 60)))
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
