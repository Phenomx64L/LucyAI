//! El idioma de la interfaz.
//!
//! CINCO IDIOMAS Y NO DOS, que son los que ofrece el instalador de la V1
//! (`SetupOverlay.svelte`: `t(es, pt, en, fr, de)`). El cockpit de la V2 se
//! quedó en español e inglés, y eso convierte la elección del instalador en una
//! promesa a medias: alguien que instala en portugués se encuentra media
//! aplicación en español. Aquí se cubren los cinco o no se cubre ninguno.
//!
//! POR TABLA CON CLAVE Y NO POR PARES EN LÍNEA. La V2 usa `t('Hola', 'Hello')`
//! con los dos textos en el sitio de la llamada, que con dos idiomas es lo más
//! cómodo que hay. Con cinco serían cinco cadenas incrustadas en cada una de las
//! cientos de llamadas de la pantalla, y el código dejaría de leerse. La tabla
//! separa «qué se dice» de «cómo se dice en cada sitio».
//!
//! LOS TEXTOS VAN EN UN ARRAY, NO EN CINCO CAMPOS. Es la parte que hace que esto
//! no se pudra: añadir un idioma cambia el tamaño del array, y entonces el
//! compilador obliga a rellenarlo en TODAS las frases. Con cinco campos con
//! nombre, un idioma nuevo se añadiría con `Default` vacío y la mitad de la
//! interfaz saldría en blanco sin que nada avisara.
//!
//! EL ESPAÑOL ES LA FUENTE. Es el idioma en el que están escritos los textos
//! originales y el que se usa cuando falta una traducción — un hueco se ve como
//! una interfaz rota, y una frase en otro idioma se entiende igual de mal pero
//! al menos dice algo.

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

    /// En el MISMO ORDEN que los textos de cada frase. Cambiarlo sin cambiar las
    /// tablas dejaría cada idioma enseñando el texto de otro.
    pub const ALL: [Lang; Self::N] = [Lang::Es, Lang::En, Lang::Pt, Lang::Fr, Lang::De];

    /// Su posición en el array de textos.
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

    /// Del código guardado, o de lo que diga el sistema. Español si no se sabe.
    ///
    /// POR PREFIJO, porque lo que se guarda es `es-MX` o `en-US`, no `es`. Un
    /// `==` exacto contra `es` haría que un `es-MX` guardado por la V1 se leyera
    /// como «no lo sé» y volviera al idioma por defecto — que casualmente es el
    /// mismo, así que el fallo no se vería hasta que alguien eligiera `pt-BR`.
    pub fn de_clave(v: &str) -> Option<Lang> {
        let v = v.trim().to_ascii_lowercase();
        Lang::ALL.into_iter().find(|l| v.starts_with(l.clave()))
    }
}

/// El idioma puesto ahora mismo, como índice.
///
/// ATÓMICO Y GLOBAL como `motion()`, y por la misma razón: lo lee cada texto de
/// cada frame desde sitios que no tienen acceso a la aplicación. Pasarlo por
/// parámetro obligaría a llevarlo por toda la pila de dibujo.
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
    /// La clave. Por familias con punto —`nav.dashboard`, `cfg.tema`— para que
    /// la tabla se pueda leer por secciones y para que ordenarla agrupe lo que
    /// va junto.
    pub clave: &'static str,
    /// En el orden de [`Lang::ALL`]. Array y no cinco campos: ver la cabecera.
    pub textos: [&'static str; Lang::N],
}

/// El texto de una clave en el idioma puesto.
///
/// DEVUELVE LA CLAVE si no existe, y eso es a propósito: un texto que falta sale
/// en pantalla como `nav.loquesea`, que es feo y se arregla en cinco segundos.
/// Devolver una cadena vacía dejaría un hueco mudo que nadie encuentra.
pub fn t(clave: &str) -> &'static str {
    let Some(f) = busca(clave) else {
        return "‹falta›";
    };
    let s = f.textos[lang().idx()];
    // Sin traducir, el español. Un hueco se lee como una interfaz rota; una
    // frase en otro idioma se entiende igual de mal pero al menos dice algo.
    if s.is_empty() {
        f.textos[0]
    } else {
        s
    }
}

/// La frase de una clave, si está.
///
/// BÚSQUEDA BINARIA sobre la tabla ordenada. Con varios cientos de frases y una
/// pantalla que las pide todas sesenta veces por segundo, un recorrido lineal
/// serían decenas de miles de comparaciones por frame. El test de abajo
/// comprueba que la tabla está ordenada, que es lo que hace válida la búsqueda.
fn busca(clave: &str) -> Option<&'static Frase> {
    FRASES
        .binary_search_by(|f| f.clave.cmp(clave))
        .ok()
        .map(|i| &FRASES[i])
}

/// Todas las frases de la interfaz, ORDENADAS POR CLAVE.
///
/// El orden no es estético: lo exige la búsqueda binaria, y hay un test que lo
/// vigila. Si añades una frase en medio, el test dice dónde.
pub const FRASES: &[Frase] = &[
    // ── La ayuda de cada módulo ──────────────────────────────────────────────
    Frase {
        clave: "ayuda.compliance",
        textos: [
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
        ],
    },
    Frase {
        clave: "ayuda.configuracion",
        textos: [
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
        ],
    },
    Frase {
        clave: "ayuda.dashboard",
        textos: [
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
        ],
    },
    Frase {
        clave: "ayuda.inventario",
        textos: [
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
        ],
    },
    Frase {
        clave: "ayuda.logviewer",
        textos: [
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
        ],
    },
    Frase {
        clave: "ayuda.memoria",
        textos: [
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
        ],
    },
    Frase {
        clave: "ayuda.nexshell",
        textos: [
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
        ],
    },
    Frase {
        clave: "ayuda.terminalia",
        textos: [
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
        ],
    },
    // ── Configuración ────────────────────────────────────────────────────────
    Frase {
        clave: "cfg.idioma",
        textos: ["Idioma", "Language", "Idioma", "Langue", "Sprache"],
    },
    // LA COBERTURA SE DICE EN LA PROPIA PANTALLA mientras sea parcial. Un
    // selector que cambia la navegación y deja el resto en español se lee como
    // una traducción rota; dicho de antemano, se lee como una traducción a
    // medias, que es lo que es. Esta frase se borra cuando no quede nada por
    // traducir, y el test `cobertura` de abajo dice cuánto falta.
    Frase {
        clave: "cfg.idioma.sub",
        textos: [
            "de la interfaz · por ahora traducidos la navegación y los textos de ayuda; \
             el resto va en camino",
            "of the interface · so far the navigation and the help texts are translated; \
             the rest is on its way",
            "da interface · por agora traduzidos a navegação e os textos de ajuda; \
             o resto está a caminho",
            "de l'interface · pour l'instant la navigation et les textes d'aide sont \
             traduits ; le reste arrive",
            "der Oberfläche · bisher sind die Navigation und die Hilfetexte übersetzt; \
             der Rest folgt",
        ],
    },
    // ── La barra lateral ─────────────────────────────────────────────────────
    //
    // Los nombres propios NO SE TRADUCEN —NexShell, Log Viewer, Terminal IA son
    // partes de Lucy con nombre, no descripciones— así que sus cinco columnas
    // son iguales a propósito. Se dejan escritas igualmente en vez de tratarlas
    // aparte: una excepción en la tabla obligaría a mirar el código para saber
    // si una fila repetida es una decisión o un olvido.
    Frase {
        clave: "nav.compliance",
        textos: ["Compliance", "Compliance", "Compliance", "Compliance", "Compliance"],
    },
    Frase {
        clave: "nav.configuracion",
        textos: ["Configuración", "Settings", "Configuração", "Réglages", "Einstellungen"],
    },
    Frase {
        clave: "nav.dashboard",
        textos: ["Dashboard", "Dashboard", "Dashboard", "Tableau de bord", "Übersicht"],
    },
    Frase {
        clave: "nav.inventario",
        textos: ["Inventario", "Inventory", "Inventário", "Inventaire", "Bestand"],
    },
    Frase {
        clave: "nav.logviewer",
        textos: ["Log Viewer", "Log Viewer", "Log Viewer", "Log Viewer", "Log Viewer"],
    },
    Frase {
        clave: "nav.memoria",
        textos: ["Memoria", "Memory", "Memória", "Mémoire", "Gedächtnis"],
    },
    Frase {
        clave: "nav.nexshell",
        textos: ["NexShell", "NexShell", "NexShell", "NexShell", "NexShell"],
    },
    Frase {
        clave: "nav.terminalia",
        textos: ["Terminal IA", "Terminal IA", "Terminal IA", "Terminal IA", "Terminal IA"],
    },
    // ── Los títulos de página ────────────────────────────────────────────────
    Frase {
        clave: "titulo.compliance",
        textos: ["Compliance", "Compliance", "Compliance", "Compliance", "Compliance"],
    },
    Frase {
        clave: "titulo.configuracion",
        textos: ["Configuración", "Settings", "Configuração", "Réglages", "Einstellungen"],
    },
    Frase {
        clave: "titulo.dashboard",
        textos: [
            "Dashboard de sistema",
            "System dashboard",
            "Painel do sistema",
            "Tableau de bord système",
            "System-Übersicht",
        ],
    },
    Frase {
        clave: "titulo.inventario",
        textos: ["Inventario", "Inventory", "Inventário", "Inventaire", "Bestand"],
    },
    Frase {
        clave: "titulo.logviewer",
        textos: [
            "Visor de logs",
            "Log viewer",
            "Visor de logs",
            "Visionneuse de logs",
            "Log-Ansicht",
        ],
    },
    Frase {
        clave: "titulo.memoria",
        textos: ["Memoria", "Memory", "Memória", "Mémoire", "Gedächtnis"],
    },
    Frase {
        clave: "titulo.nexshell",
        textos: ["NexShell", "NexShell", "NexShell", "NexShell", "NexShell"],
    },
    Frase {
        clave: "titulo.terminalia",
        textos: ["Terminal IA", "Terminal IA", "Terminal IA", "Terminal IA", "Terminal IA"],
    },
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
        // Una clave fuera de orden no falla: hace que ESA frase no se encuentre
        // nunca, y sale «‹falta›» en pantalla sin que nada diga por qué.
        for par in FRASES.windows(2) {
            assert!(
                par[0].clave < par[1].clave,
                "«{}» va después de «{}» y la tabla tiene que ir ordenada",
                par[0].clave,
                par[1].clave
            );
        }
    }

    #[test]
    fn ninguna_frase_se_queda_sin_español() {
        // El español es la fuente y el respaldo. Sin él, una clave sin traducir
        // no tiene a qué caer.
        for f in FRASES {
            assert!(
                !f.textos[0].trim().is_empty(),
                "«{}» no tiene texto en español, que es de donde salen los demás",
                f.clave
            );
        }
    }

    #[test]
    fn se_encuentran_todas_las_claves() {
        let _g = cerrojo();
        for f in FRASES {
            for l in Lang::ALL {
                set(l);
                assert_ne!(
                    t(f.clave),
                    "‹falta›",
                    "«{}» no se encuentra en {}",
                    f.clave,
                    l.nombre()
                );
            }
        }
        set(Lang::Es);
    }

    #[test]
    fn una_traduccion_vacia_cae_al_español_y_no_deja_un_hueco() {
        let f = Frase { clave: "x", textos: ["hola", "", "", "", ""] };
        // Se comprueba la regla sobre la propia frase, sin tocar la tabla: lo
        // que importa es que un texto vacío nunca llega a pintarse.
        for l in Lang::ALL {
            let s = f.textos[l.idx()];
            let visto = if s.is_empty() { f.textos[0] } else { s };
            assert_eq!(visto, "hola", "{} se queda en blanco", l.nombre());
        }
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
    fn cambiar_de_idioma_cambia_lo_que_se_lee() {
        let _g = cerrojo();
        set(Lang::Es);
        assert_eq!(t("titulo.configuracion"), "Configuración");
        set(Lang::De);
        assert_eq!(t("titulo.configuracion"), "Einstellungen");
        set(Lang::Es);
    }

    #[test]
    fn el_orden_de_los_idiomas_casa_con_el_de_los_textos() {
        // Si alguien reordena `Lang::ALL` sin tocar las tablas, cada idioma
        // enseñaría el texto de otro — y en cinco columnas de texto plano eso no
        // se ve leyendo el diff.
        for (i, l) in Lang::ALL.into_iter().enumerate() {
            assert_eq!(l.idx(), i, "{} no está en su sitio", l.nombre());
        }
    }
}
