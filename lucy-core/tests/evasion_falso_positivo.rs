//! El guardrail bloquea ficheros normales al escribirlos.
//!
//! REPRODUCIDO DESDE UN USO REAL. El operador pidió a Lucy que corrigiera un
//! proyecto de GoAnywhere —un XML— y el artefacto salió bloqueado con «El
//! comando usa una forma conocida de esquivar filtros, no una forma de
//! escribirlo». El fichero no tiene nada raro: es XML con `<delete>` dentro.
//!
//! El patrón `[\t\r] .* (?: format | del | rmdir )` de `EVASION_CMD` está
//! escrito para cazar la ofuscación de `cmd.exe` —meter un tabulador en medio
//! de `del /s` para que un filtro de subcadenas no lo vea— pero se escribió sin
//! límites de palabra y con `.*`. Resultado: en CUALQUIER texto con saltos de
//! línea de Windows, un `\r` seguido de las letras `d`,`e`,`l` en cualquier
//! parte de la línea basta.
//!
//! O sea que casa con `<delete>`, con `modelo`, con `delimiter`, con `Delta` y
//! con `formato`. Y `writefile` pasa TODO el contenido por ahí.

/// Un trozo del XML real, con los saltos de línea de Windows que tenía.
const XML_REAL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\
     <project name=\"INCODE_S3_COPY\" mainModule=\"Main\" version=\"2.0\">\r\n\
     \t<module name=\"Main\">\r\n\
     \t\t<delete version=\"1.0\" disabled=\"false\">\r\n\
     \t\t\t<fileset dir=\"${ruta_origen}\" recursive=\"true\" includeItems=\"files\" />\r\n\
     \t\t</delete>\r\n\
     \t</module>\r\n\
     </project>\r\n";

#[test]
fn un_xml_normal_no_es_una_evasion_de_filtros() {
    let s = lucy_core::guard::scan(XML_REAL, lucy_core::guard::Role::Assistant);
    assert_ne!(
        s.decision,
        lucy_core::guard::Decision::Block,
        "un XML con <delete> se bloquea como evasión de filtros: {}",
        s.reason
    );
}

#[test]
fn las_palabras_corrientes_que_llevan_del_dentro_tampoco() {
    // Ninguna de éstas es un comando, y las cuatro llevan `del` o `format`
    // dentro de una palabra normal. Con `\r\n` delante, que es como llega
    // cualquier fichero escrito en Windows.
    for texto in [
        "línea uno\r\nel modelo elegido es gemini",
        "cabecera\r\nformato de fecha: ISO-8601",
        "uno\r\n<delimiter>;</delimiter>",
        "uno\r\nDelta de configuración",
    ] {
        let s = lucy_core::guard::scan(texto, lucy_core::guard::Role::Assistant);
        assert_ne!(
            s.decision,
            lucy_core::guard::Decision::Block,
            "bloqueado texto normal: {texto:?} — {}",
            s.reason
        );
    }
}

#[test]
fn la_evasion_de_verdad_se_sigue_cazando() {
    // Lo que el patrón vino a cazar TIENE que seguir cazándose. Un arreglo que
    // quita el falso positivo abriendo la puerta a lo real no es un arreglo.
    for ataque in [
        "del\t/s /q C:\\datos",
        "format\t/y D:",
        "rmdir\t/s /q C:\\Windows",
        // El circunflejo de `cmd`: se ejecuta igual y no lo ve un filtro de
        // subcadenas que busque «del».
        "d^el /s /q C:\\datos",
        "for^mat /y D:",
    ] {
        let s = lucy_core::guard::scan(ataque, lucy_core::guard::Role::Assistant);
        assert_eq!(
            s.decision,
            lucy_core::guard::Decision::Block,
            "se escapó una evasión de verdad: {ataque:?}"
        );
    }
}
