// main_console.rs - versão camuflada
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    println!("=== CPU Stress Test (Console) ===");
    let total_cores = num_cpus::get();
    println!("Núcleos detectados: {}", total_cores);

    let percent = 50;
    let num_workers = ((total_cores as f32 * percent as f32 / 100.0).ceil() as usize)
        .min(total_cores)
        .max(1);
    let cycle_ms = 100;
    let work_ms = (cycle_ms * percent as u64 / 100).max(1);
    let idle_ms = cycle_ms - work_ms;

    println!("Usando {} workers", num_workers);
    println!("Ciclo: {}ms carga / {}ms pausa", work_ms, idle_ms);
    println!("Pressione Ctrl+C para parar.\n");

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\nParando teste...");
        r.store(false, Ordering::SeqCst);
    }).expect("Erro ao configurar Ctrl+C");

    let mut handles = vec![];

    for id in 0..num_workers {
        let running = running.clone();
        let work_ms = work_ms;
        let idle_ms = idle_ms;

        handles.push(thread::spawn(move || {
            let mut counter = id as u64;
            let mut total_ops = 0u64;
            let start_total = Instant::now();

            let mut last_report = Instant::now();
            let mut last_ops = 0u64;

            while running.load(Ordering::SeqCst) {
                let cycle_start = Instant::now();
                while cycle_start.elapsed().as_millis() < work_ms as u128 {
                    // Simulação de carga computacional
                    let _ = counter.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(0xbf58476d1ce4e5b9);
                    counter = counter.wrapping_add(1);
                    total_ops += 1;
                }
                thread::sleep(Duration::from_millis(idle_ms));

                let now = Instant::now();
                if now.duration_since(last_report).as_secs_f64() >= 1.0 {
                    let ops_in_period = total_ops - last_ops;
                    let elapsed_secs = now.duration_since(last_report).as_secs_f64();
                    let ops_rate = ops_in_period as f64 / elapsed_secs;
                    println!("Worker {}: {:.2} ops/s (total: {} ops)", id, ops_rate, total_ops);
                    last_report = now;
                    last_ops = total_ops;
                }
            }

            let total_elapsed = start_total.elapsed().as_secs_f64();
            let avg_ops = total_ops as f64 / total_elapsed;
            println!("Worker {} finalizado. Média: {:.2} ops/s, total ops: {}", id, avg_ops, total_ops);
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }
    println!("Programa encerrado.");
}
