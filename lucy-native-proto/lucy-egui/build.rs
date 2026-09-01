//! Mete el icono y los datos de versión DENTRO del ejecutable.
//!
//! POR QUÉ HACE FALTA, y por qué no basta con el icono que ya se ponía. El
//! `main` pasa un `egui::IconData` al crear la ventana, y eso resuelve el icono
//! de la VENTANA en ejecución. El del FICHERO es otra cosa: es un recurso Win32
//! que el enlazador incrusta en el `.exe`, y sin él Windows enseña el icono
//! genérico de aplicación en el Explorador, en el acceso directo del
//! escritorio, en el del menú de inicio y al fijar en la barra de tareas.
//!
//! Se vio al instalar la 2.0.1: la aplicación arrancaba bien y con su icono en
//! la ventana, pero los tres accesos directos que crea el instalador salían con
//! el rectángulo azul de «programa sin icono». Un icono genérico en el
//! escritorio es de las cosas que hacen que un instalador parezca roto aunque
//! todo lo demás funcione.
//!
//! Y DE PASO LOS DATOS DE VERSIÓN. Sin ellos, la pestaña «Detalles» de las
//! propiedades del fichero sale vacía y el Administrador de tareas no tiene
//! nada que enseñar en la columna de descripción. Es lo que mira alguien que
//! encuentra un proceso desconocido comiéndose la CPU.

fn main() {
    // Solo en Windows: `winresource` no hace nada en otras plataformas, pero
    // pedirle que lea un .ico que no existe sí falla.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    println!("cargo:rerun-if-changed=assets/lucy.ico");

    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/lucy.ico")
        .set("ProductName", "Lucy")
        .set("FileDescription", "Lucy — asistente de administración de sistemas")
        .set("CompanyName", "Iván Eduardo Luna")
        .set("LegalCopyright", "Copyright © 2026 Iván Eduardo Luna. GPLv3.")
        .set("OriginalFilename", "lucy-egui.exe");

    // UN FALLO AQUÍ NO PUEDE PARAR LA COMPILACIÓN. `winresource` necesita las
    // herramientas de recursos del sistema, y en una máquina donde no estén
    // —una compilación cruzada, un contenedor pelado— quedarse sin binario por
    // no poder poner un icono sería un mal negocio. Se avisa y se sigue.
    if let Err(e) = res.compile() {
        println!("cargo:warning=sin icono ni datos de versión en el .exe: {e}");
    }
}
