//! Comprueba el canal de avisos de Lucy.
//!
//! Manda un aviso de prueba y dice lo que se puede saber de él. Que el globo
//! aparezca en pantalla NO se puede comprobar desde aquí por ninguna vía —lo
//! intenté por tres, y las tres contestan lo mismo pase lo que pase— así que la
//! última palabra es de quien esté delante.
fn main() {
    let d = lucy_core::notify::diagnostico();
    println!("se lanzó:        {}", d.se_lanzo);
    println!("queda archivado: {}", d.queda_en_el_centro);
    println!();
    println!("{}", d.detalle);
}
