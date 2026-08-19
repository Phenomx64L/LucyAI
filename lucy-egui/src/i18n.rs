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
    f!("Activadas", "On", "Ativadas", "Activées", "Ein"),
    f!("Activado", "On", "Ativado", "Activé", "Ein"),
    f!("Activo", "On", "Ativo", "Actif", "Ein"),
    f!(
        "Actualizar ahora",
        "Update now",
        "Atualizar agora",
        "Actualiser maintenant",
        "Jetzt aktualisieren",
    ),
    f!(
        "Ahora mismo resuelve a",
        "Currently resolves to",
        "Agora resolve para",
        "Actuellement résolu en",
        "Aktuell aufgelöst als",
    ),
    f!("Animaciones", "Animations", "Animações", "Animations", "Animationen"),
    f!("Apagadas", "Off", "Desligadas", "Désactivées", "Aus"),
    f!("Apagado", "Off", "Desligado", "Désactivé", "Aus"),
    f!(
        "Avisar si el modelo se queda corto",
        "Warn if the model falls short",
        "Avisar se o modelo ficar aquém",
        "Prévenir si le modèle est trop juste",
        "Warnen, wenn das Modell nicht reicht",
    ),
    f!("Claves API", "API keys", "Chaves API", "Clés API", "API-Schlüssel"),
    f!(
        "Color de acento",
        "Accent colour",
        "Cor de destaque",
        "Couleur d'accent",
        "Akzentfarbe",
    ),
    f!("Compliance", "Compliance", "Conformidade", "Conformité", "Compliance"),
    f!(
        "Comprobar que responde y con qué sistema",
        "Check it responds and on what system",
        "Verificar se responde e com que sistema",
        "Vérifier qu'elle répond et avec quel système",
        "Erreichbarkeit und System prüfen",
    ),
    f!("Conectar", "Connect", "Ligar", "Connecter", "Verbinden"),
    f!("Configuración", "Settings", "Configuração", "Réglages", "Einstellungen"),
    f!("Copia de seguridad", "Backup", "Cópia de segurança", "Sauvegarde", "Sicherung"),
    f!("Copiar la ruta", "Copy path", "Copiar o caminho", "Copier le chemin", "Pfad kopieren"),
    f!("Copiar la salida", "Copy output", "Copiar a saída", "Copier la sortie", "Ausgabe kopieren"),
    f!(
        "Copiar toda la salida",
        "Copy all output",
        "Copiar toda a saída",
        "Copier toute la sortie",
        "Ganze Ausgabe kopieren",
    ),
    f!(
        "Cristales y patrones",
        "Crystals and patterns",
        "Cristais e padrões",
        "Cristaux et motifs",
        "Kristalle und Muster",
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
    f!("Dashboard", "Dashboard", "Painel", "Tableau de bord", "Übersicht"),
    f!(
        "Dashboard de sistema",
        "System dashboard",
        "Painel do sistema",
        "Tableau de bord système",
        "System-Übersicht",
    ),
    f!(
        "Desinstalar: borra la carpeta del skill",
        "Uninstall: deletes the skill folder",
        "Desinstalar: apaga a pasta do skill",
        "Désinstaller : supprime le dossier du skill",
        "Deinstallieren: löscht den Skill-Ordner",
    ),
    f!("Equipo", "Machine", "Máquina", "Machine", "Rechner"),
    f!("Este equipo", "This machine", "Esta máquina", "Cette machine", "Dieser Rechner"),
    f!("Guardar", "Save", "Guardar", "Enregistrer", "Speichern"),
    f!(
        "Guardar copia…",
        "Save a copy…",
        "Guardar cópia…",
        "Enregistrer une copie…",
        "Kopie speichern…",
    ),
    f!("Idioma", "Language", "Idioma", "Langue", "Sprache"),
    f!("Instalar…", "Install…", "Instalar…", "Installer…", "Installieren…"),
    f!("Interfaz", "Interface", "Interface", "Interface", "Oberfläche"),
    f!("Inventario", "Inventory", "Inventário", "Inventaire", "Bestand"),
    f!(
        "La memoria en disco",
        "Memory on disk",
        "A memória em disco",
        "La mémoire sur disque",
        "Das Gedächtnis auf der Festplatte",
    ),
    f!("Limpiar", "Clear", "Limpar", "Effacer", "Leeren"),
    f!(
        "Limpiar la pantalla",
        "Clear screen",
        "Limpar o ecrã",
        "Effacer l'écran",
        "Bildschirm leeren",
    ),
    f!("Listo para operar", "Ready to operate", "Pronto a operar", "Opérationnel", "Einsatzbereit"),
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
    f!("Memoria", "Memory", "Memória", "Mémoire", "Gedächtnis"),
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
        "Ollama · modelos locales",
        "Ollama · local models",
        "Ollama · modelos locais",
        "Ollama · modèles locaux",
        "Ollama · lokale Modelle",
    ),
    f!("Operador", "Operator", "Operador", "Opérateur", "Operator"),
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
    f!("Privilegios", "Privileges", "Privilégios", "Privilèges", "Rechte"),
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
    f!(
        "Recuerdo por significado",
        "Recall by meaning",
        "Recordação por significado",
        "Souvenir par le sens",
        "Erinnern nach Bedeutung",
    ),
    f!(
        "Sin equipos remotos dados de alta",
        "No remote machines registered",
        "Sem máquinas remotas registadas",
        "Aucune machine distante enregistrée",
        "Keine Remote-Rechner eingetragen",
    ),
    f!("Sistema", "System", "Sistema", "Système", "System"),
    f!("Skills", "Skills", "Skills", "Skills", "Skills"),
    f!("Tema", "Theme", "Tema", "Thème", "Design"),
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
    f!(
        "Trozos sin vector",
        "Chunks with no vector",
        "Fragmentos sem vetor",
        "Fragments sans vecteur",
        "Textstücke ohne Vektor",
    ),
    f!("Tu nombre", "Your name", "O teu nome", "Ton nom", "Dein Name"),
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
    f!("Visor de logs", "Log viewer", "Visor de logs", "Visionneuse de logs", "Log-Ansicht"),
    // ── La ayuda de cada módulo ──────────────────────────────────────────────
    f!(
        "antes de mandar una tarea exigente · no cambia el modelo por ti",
        "before you send a demanding task · doesn't switch the model for you",
        "antes de enviar uma tarefa exigente · não muda o modelo por ti",
        "avant d'envoyer une tâche exigeante · ne change pas le modèle à ta place",
        "vor einer anspruchsvollen Aufgabe · wechselt das Modell nicht für dich",
    ),
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
        "escritura progresiva y transiciones · LUCY_NO_MOTION=1 las apaga al arrancar",
        "progressive typing and transitions · LUCY_NO_MOTION=1 turns them off at startup",
        "escrita progressiva e transições · LUCY_NO_MOTION=1 desliga-as ao arrancar",
        "écriture progressive et transitions · LUCY_NO_MOTION=1 les désactive au démarrage",
        "schrittweise Ausgabe und Übergänge · LUCY_NO_MOTION=1 schaltet sie beim Start aus",
    ),
    f!(
        "lo que se ilumina: navegación, progreso, hecho",
        "what lights up: navigation, progress, done",
        "o que se ilumina: navegação, progresso, concluído",
        "ce qui s'allume : navigation, progression, terminé",
        "was hervorgehoben wird: Navigation, Fortschritt, erledigt",
    ),
    f!("no vale", "not valid", "não serve", "invalide", "ungültig"),
    f!("pegar clave", "paste key", "colar chave", "coller la clé", "Schlüssel einfügen"),
    f!(
        "si se deja vacío usa el usuario de Windows, que es una cuenta y no un nombre",
        "if left empty it uses the Windows user, which is an account and not a name",
        "se ficar vazio usa o utilizador do Windows, que é uma conta e não um nome",
        "si tu le laisses vide, on prend l'utilisateur Windows, qui est un compte et pas un nom",
        "wenn leer, gilt der Windows-Benutzer, und das ist ein Konto, kein Name",
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
    f!(
        "vuelve a contar lo de arriba",
        "counts everything above again",
        "volta a contar o de cima",
        "recompte ce qu'il y a au-dessus",
        "zählt die Werte oben neu",
    ),
    f!("válida", "valid", "válida", "valide", "gültig"),
    f!("↻ Recontar", "↻ Recount", "↻ Recontar", "↻ Recompter", "↻ Neu zählen"),
    f!("↻ Redetectar", "↻ Redetect", "↻ Redetetar", "↻ Redétecter", "↻ Neu erkennen"),
    f!("■ Detener", "■ Stop", "■ Parar", "■ Arrêter", "■ Stopp"),
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
