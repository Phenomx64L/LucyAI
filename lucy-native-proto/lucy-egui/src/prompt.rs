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

/// Lo que hay que recordar para contestar a esto.
///
/// EL MECANISMO SE FUE AL NÚCLEO, y aquí queda la política: cuánto presupuesto
/// darle según el modelo. Estaba entero aquí, en diez líneas que buscaban solo
/// entre memorias con vectores y devolvían vacío si el embebedor no contestaba —
/// o sea que en una máquina sin Ollama, Lucy no recordaba NADA nunca y el
/// síntoma era simplemente que parecía tener mala memoria.
///
/// `lucy_core::memories::recall` busca por tres caminos: memorias, trozos de
/// documento ingerido, y palabras cuando no hay vectores. Que viva allí es lo que
/// hace que el shell y la app recuerden lo mismo — con esto aquí, cada frontend
/// tenía su propia idea de qué viene al caso.
///
/// SIGUE FALLANDO EN SILENCIO. Si no hay nada que recordar, la orden se manda
/// igual: convertirlo en un error visible castigaría al operador por una función
/// que ni pidió.
pub fn recall(query: &str, weak: bool) -> lucy_core::memories::Recuerdo {
    // Un modelo flojo se ahoga con el prompt entero y contesta en prosa sin
    // emitir una sola etiqueta; recortar lo que se le recuerda es lo primero que
    // le deja sitio para lo que se le está preguntando.
    lucy_core::memories::recall(query, if weak { 2 } else { 5 })
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
