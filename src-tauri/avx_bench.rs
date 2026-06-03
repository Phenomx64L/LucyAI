
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let size = 100_000_000;
    let a = vec![1.1f32; size];
    let b = vec![2.2f32; size];
    
    let start = Instant::now();
    let mut sum = 0.0f32;
    
    // Forzamos el cálculo matricial. LLVM vectorizará este bucle si la arquitectura lo permite.
    for (&x, &y) in a.iter().zip(b.iter()) {
        sum += x * y;
    }
    
    black_box(sum);
    
    println!("Suma total: {:.2} | Tiempo de procesamiento: {:?}", sum, start.elapsed());
}
