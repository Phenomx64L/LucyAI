//! Le pide al modelo local que redacte avisos de verdad y enseña qué sale.
//!
//! Es la única forma de saber si un modelo de este tamaño aporta algo o si la
//! plantilla ya era mejor. Enseña además cuál se ACEPTA y cuál se rechaza por
//! traer una cifra que no se le había dado.
use lucy_core::redacta::{redacta, Material};

fn main() {
    // La redacción viene APAGADA de fábrica (ver `redacta::ACTIVA`). Este
    // ejemplo existe justamente para poder decidir si encenderla, así que la
    // enciende él.
    lucy_core::redacta::pon_activa(true);
    let modelos = lucy_core::chat::list_models();
    println!("modelos en Ollama: {modelos:?}");
    match lucy_core::suggest::elige(&modelos) {
        Some(m) => println!("el vigilante usaría: {m}\n"),
        None => {
            println!("ninguno sirve: siempre saldría la plantilla.");
            return;
        }
    }

    let casos = [
        Material {
            titulo: "Disco C:\\ casi lleno".into(),
            cuerpo: "C:\\ al 94 % — quedan 12.3 GB de 500.0.".into(),
            motivo: "estaba mal y ha ido a peor".into(),
            equipo: String::new(),
        },
        Material {
            titulo: "2 servicios automáticos han fallado".into(),
            cuerpo: "MSSQLSERVER, SQLSERVERAGENT".into(),
            motivo: "acaba de empezar".into(),
            equipo: "SRV-FS01".into(),
        },
        Material {
            titulo: "Resuelto: Servicios automáticos".into(),
            cuerpo: "Ya no hay ningún servicio automático fallado.".into(),
            motivo: "estaba mal y se ha arreglado".into(),
            equipo: String::new(),
        },
        Material {
            titulo: "Memoria alta".into(),
            cuerpo: "La memoria va al 93 % — 29.8 de 32.0 GB.".into(),
            motivo: "sigue igual desde hace horas".into(),
            equipo: String::new(),
        },
    ];

    for c in &casos {
        let t0 = std::time::Instant::now();
        let r = redacta(c);
        let ms = t0.elapsed().as_millis();
        println!("── plantilla ─────────────────────────────────────────");
        println!("   {}", c.titulo);
        println!("   {}", c.cuerpo);
        match r {
            Some((t, b)) => {
                println!("── redactado ({ms} ms) ────────────────────────────────");
                println!("   {t}");
                println!("   {b}");
            }
            None => println!("── RECHAZADO ({ms} ms): sale la plantilla ────────────"),
        }
        println!();
    }
}
