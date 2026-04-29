use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    println!("=== Minerador CPU (Modo Console) ===");
    let total_cores = num_cpus::get();
    println!("Núcleos detectados: {}", total_cores);

    let percent = 50;
    let num_threads = ((total_cores as f32 * percent as f32 / 100.0).ceil() as usize)
        .min(total_cores)
        .max(1);
    let cycle_ms = 100;
    let work_ms = (cycle_ms * percent as u64 / 100).max(1);
    let idle_ms = cycle_ms - work_ms;

    println!("Usando {} threads", num_threads);
    println!("Ciclo: {}ms trabalho / {}ms pausa", work_ms, idle_ms);
    println!("Pressione Ctrl+C para parar.\n");

    let running = Arc::new(AtomicBool:: new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\nParando minerador...");
        r.store(false, Ordering::SeqCst);
    }).expect("Erro ao configurar Ctrl+C");

    let mut handles = vec![];

    for id in 0..num_threads {
        let running = running.clone();
        let work_ms = work_ms;
        let idle_ms = idle_ms;

        handles.push(thread::spawn(move || {
            let mut nonce = id as u64;
            let mut total_hashes = 0u64;
            let start_total = Instant::now();

            let mut last_report = Instant::now();
            let mut last_hashes = 0u64;

            while running.load(Ordering::SeqCst) {
                let cycle_start = Instant::now();
                while cycle_start.elapsed().as_millis() < work_ms as u128 {
                    let _hash = nonce.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(0xbf58476d1ce4e5b9);
                    nonce = nonce.wrapping_add(1);
                    total_hashes += 1;
                }
                thread::sleep(Duration::from_millis(idle_ms));

                let now = Instant::now();
                if now.duration_since(last_report).as_secs_f64() >= 1.0 {
                    let hashes_in_period = total_hashes - last_hashes;
                    let elapsed_secs = now.duration_since(last_report).as_secs_f64();
                    let hashrate = hashes_in_period as f64 / elapsed_secs;
                    println!("Thread {}: {:.2} H/s (total: {} hashes)", id, hashrate, total_hashes);
                    last_report = now;
                    last_hashes = total_hashes;
                }
            }

            let total_elapsed = start_total.elapsed().as_secs_f64();
            let avg_hashrate = total_hashes as f64 / total_elapsed;
            println!("Thread {} finalizada. Média: {:.2} H/s, total hashes: {}", id, avg_hashrate, total_hashes);
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }
    println!("Programa encerrado.");
}
