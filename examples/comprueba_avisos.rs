//! Comprueba de verdad si los avisos de Lucy llegan a la pantalla.
//!
//! No pregunta a la API —que miente— sino al centro de notificaciones.
fn main() {
    let d = lucy_core::notify::diagnostico();
    println!("entrega: {}", d.entrega);
    println!("detalle: {}", d.detalle);
}
