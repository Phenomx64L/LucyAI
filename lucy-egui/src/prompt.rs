//! Lo que este shell le aporta al prompt por secciones del núcleo.
//!
//! El prompt en sí —quién es Lucy, sus reglas, el estado del equipo, la marca de
//! caché— vive en `lucy_core::prompt`, porque es lo mismo se mire desde donde se
//! mire. Aquí queda lo que solo esta ventana sabe: qué memorias vienen al caso
//! para la orden que se acaba de escribir, y qué equipos remotos hay
//! configurados en este perfil.
//!
//! ANTES ESTABA TODO AQUÍ, en un `format!` de doscientas líneas. Se movió cuando
//! quedó claro que la mitad de sus bloques viajaban vacíos en cada turno: la
//! sección de servicios diciendo "ninguno", la de log diciendo que no había log.
//! Un bloque que no dice nada enseña al modelo a saltarse ese encabezado, y el
//! día que sí trae algo tampoco lo mira.

use std::fmt::Write;

/// Cuántas memorias se recuerdan por turno.
///
/// Cinco. Con más, el prompt se llena de cosas tangencialmente parecidas y el
/// modelo empieza a construir sobre lo que se le recordó en vez de sobre lo que
/// se le preguntó — el fallo típico de la recuperación semántica generosa.
const RECALL_LIMIT: usize = 5;

/// Parecido mínimo para que una memoria entre.
///
/// Más alto que el 0.25 del buscador de la vista de Memoria, y a propósito: allí
/// el operador está buscando y juzga él; aquí se le mete al modelo sin que nadie
/// lo mire, y una memoria irrelevante inyectada en silencio es peor que ninguna.
const RECALL_MIN: f32 = 0.4;

/// Las memorias parecidas a esta orden, ya formateadas. Vacío = ninguna.
///
/// Es lo que hace que Lucy sea la misma entre sesiones: sin esto, cada arranque
/// empieza sin saber nada de lo que ya se resolvió en esta máquina.
///
/// FALLA EN SILENCIO A PROPÓSITO. La búsqueda necesita el embebedor local, y si
/// Ollama no está corriendo no hay recuerdo posible — pero tampoco hay nada roto
/// que anunciar: la orden se manda igual, solo que sin memoria. Convertir eso en
/// un error visible castigaría al operador por una función que ni pidió.
pub fn recall(query: &str) -> String {
    let Ok((hits, _)) = lucy_core::vectors::search(query, "memory", RECALL_LIMIT, RECALL_MIN)
    else {
        return String::new();
    };
    let mut s = String::new();
    for h in &hits {
        let _ = writeln!(s, "- {}", h.text.replace('\n', " "));
    }
    s
}

/// Los equipos remotos configurados, ya formateados. Vacío = no hay ninguno.
///
/// Van con su id porque es lo que identificaría una ejecución remota el día que
/// se migre; hoy la sección del núcleo solo los nombra para que Lucy sepa que
/// existen y no proponga correr aquí algo que era para allá.
pub fn hosts_block(hosts: &[lucy_core::hosts::Host]) -> String {
    let mut s = String::new();
    for h in hosts {
        let _ = writeln!(
            s,
            "- {} (id {}) — {} en {}",
            h.name,
            h.id,
            h.protocol.label(),
            if h.host.is_empty() { "sin dirección" } else { &h.host }
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucy_core::hosts::Host;

    #[test]
    fn el_bloque_de_hosts_lleva_el_id_y_el_transporte() {
        // El id es lo que identificaría la ejecución remota cuando se migre, y
        // el transporte es lo que decide si un comando es PowerShell o bash.
        let hs = [Host {
            name: "Servidor de archivos".into(),
            host: "10.0.0.5".into(),
            username: "admin".into(),
            ..Host::nuevo(lucy_core::hosts::Protocol::Winrm, 0)
        }];
        let hs = [Host { id: "srv01".into(), ..hs[0].clone() }];
        let b = hosts_block(&hs);
        assert!(b.contains("Servidor de archivos"));
        assert!(b.contains("id srv01"));
        assert!(b.contains("WinRM"));
        assert!(b.contains("10.0.0.5"));
    }

    #[test]
    fn sin_hosts_el_bloque_esta_vacio_y_la_seccion_no_sale() {
        // Es lo que hace que la sección de enrutado no aparezca: `relevant`
        // mira si esta cadena está vacía.
        assert!(hosts_block(&[]).is_empty());
    }

    #[test]
    fn un_host_sin_direccion_lo_dice_en_vez_de_dejar_un_hueco() {
        let hs = [Host {
            name: "Sin IP".into(),
            ..Host::nuevo(lucy_core::hosts::Protocol::Ssh, 0)
        }];
        assert!(hosts_block(&hs).contains("sin dirección"));
    }
}
