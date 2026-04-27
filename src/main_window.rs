#![windows_subsystem = "windows"]

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

fn log(msg: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("miner.log")
        .unwrap_or_else(|_| panic!("Não foi possível abrir miner.log"));
    writeln!(file, "{}", msg).ok();
}

fn main() {
    log("=== Minerador CPU (Modo Silencioso) ===");
    let total_cores = num_cpus::get();
    log(&format!("Núcleos detectados: {}", total_cores));

    let percent = 50;
    let num_threads = ((total_cores as f32 * percent as f32 / 100.0).ceil() as usize)
        .min(total_cores)
        .max(1);
    let cycle_ms = 100;
    let work_ms = (cycle_ms * percent as u64 / 100).max(1);
    let idle_ms = cycle_ms - work_ms;

    log(&format!("Usando {} threads, ciclo {}ms trab / {}ms pausa", num_threads, work_ms, idle_ms));

    let running = Arc::new(AtomicBool::new(true));

    let mut handles = vec![];
    for id in 0..num_threads {
        let running = running.clone();
        let work_ms = work_ms;
        let idle_ms = idle_ms;
        handles.push(thread::spawn(move || {
            let mut nonce = id as u64;
            let mut count = 0;
            let start = Instant::now();
            while running.load(Ordering::SeqCst) {
                let cycle_start = Instant::now();
                while cycle_start.elapsed().as_millis() < work_ms as u128 {
                    let _hash = nonce.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(0xbf58476d1ce4e5b9);
                    nonce = nonce.wrapping_add(1);
                    count += 1;
                }
                thread::sleep(Duration::from_millis(idle_ms));
                if count % 100000 == 0 {
                    let elapsed = start.elapsed().as_secs_f64();
                    let hashrate = count as f64 / elapsed;
                    log(&format!("Thread {}: {:.2} H/s", id, hashrate));
                }
            }
            log(&format!("Thread {} terminada. Total hashes: {}", id, count));
        }));
    }

    // Mantém a thread principal viva (o programa só termina quando for morto)
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(10));
    }

    for handle in handles {
        let _ = handle.join();
    }
    log("Programa encerrado.");
}
