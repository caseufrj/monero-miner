// minerador monero real em rust
// uso: cargo run --release -- --pool pool.supportxmr.com:5555 --wallet SUA_CARTEIRA --cpu-percent 50

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use std::net::TcpStream;
use std::io::{BufRead, BufReader, Write};
use serde_json::{json, Value};
use randomx_rs::RandomX;
use hex;
use clap::Parser;

// ---------- CLI ----------
#[derive(Parser)]
#[command(name = "monero-miner")]
#[command(about = "Minerador Monero (RandomX) com controle de uso da CPU")]
struct Args {
    /// URL da pool (ex: pool.supportxmr.com:5555)
    #[arg(short, long)]
    pool: String,

    /// Endereço da carteira Monero
    #[arg(short, long)]
    wallet: String,

    /// Percentual da CPU a usar (10-100)
    #[arg(short, long, default_value = "50")]
    cpu_percent: u8,
}

// ---------- Cliente Stratum ----------
struct StratumClient {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    job_id: String,
    blob: String,
    target: String,
}

impl StratumClient {
    fn connect(pool_url: &str, wallet: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut stream = TcpStream::connect(pool_url)?;
        let reader = BufReader::new(stream.try_clone()?);
        let mut client = StratumClient {
            stream,
            reader,
            job_id: String::new(),
            blob: String::new(),
            target: String::new(),
        };
        // Envia subscribe e authorize
        client.send_request("mining.subscribe", json!(["monero-miner-rust/1.0"]))?;
        client.send_request("mining.authorize", json!([wallet, "x"]))?;
        // Lê primeira resposta (ignora)
        let _ = client.read_response()?;
        Ok(client)
    }

    fn send_request(&mut self, method: &str, params: Value) -> Result<(), Box<dyn std::error::Error>> {
        let id = 1;
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params, "id": id});
        self.stream.write_all(msg.to_string().as_bytes())?;
        self.stream.write_all(b"\n")?;
        self.stream.flush()?;
        Ok(())
    }

    fn read_response(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        let resp: Value = serde_json::from_str(&line)?;
        Ok(resp)
    }

    fn get_job(&mut self) -> Result<(String, String, String), Box<dyn std::error::Error>> {
        loop {
            let resp = self.read_response()?;
            if let Some(params) = resp.get("params") {
                // notificação de job: {"method":"mining.notify","params":[...]}
                if resp.get("method").and_then(|m| m.as_str()) == Some("mining.notify") {
                    let job_id = params[0].as_str().ok_or("missing job_id")?.to_string();
                    let blob = params[1].as_str().ok_or("missing blob")?.to_string();
                    let target = params[2].as_str().ok_or("missing target")?.to_string();
                    return Ok((job_id, blob, target));
                }
            }
            // resposta a submit, etc. ignoramos
        }
    }

    fn submit(&mut self, job_id: &str, nonce: u32, hash: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let nonce_hex = hex::encode(&nonce.to_le_bytes());
        let hash_hex = hex::encode(hash);
        let params = json!([job_id, nonce_hex, hash_hex]);
        self.send_request("mining.submit", params)?;
        Ok(())
    }
}

// ---------- Minerador ----------
struct Miner {
    config: MinerConfig,
    running: Arc<AtomicBool>,
}

struct MinerConfig {
    num_threads: usize,
    work_ms: u64,
    idle_ms: u64,
    wallet: String,
    pool_url: String,
}

impl Miner {
    fn new(wallet: String, pool_url: String, cpu_percent: u8) -> Self {
        let total_cores = num_cpus::get();
        let num_threads = ((total_cores as f32 * cpu_percent as f32 / 100.0).ceil() as usize)
            .min(total_cores)
            .max(1);
        let cycle_ms = 100;
        let work_ms = (cycle_ms * cpu_percent as u64 / 100).max(1);
        let idle_ms = cycle_ms - work_ms;

        println!("Configuração: {} threads", num_threads);
        println!("Ciclo: {}ms trabalho / {}ms pausa", work_ms, idle_ms);

        Miner {
            config: MinerConfig {
                num_threads,
                work_ms,
                idle_ms,
                wallet,
                pool_url,
            },
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    fn start(&self) {
        let running = self.running.clone();
        let r = running.clone();
        ctrlc::set_handler(move || {
            println!("\nRecebido sinal de parada. Encerrando...");
            r.store(false, Ordering::SeqCst);
        }).expect("Erro ao configurar Ctrl+C");

        let mut handles = vec![];
        for thread_id in 0..self.config.num_threads {
            let running = running.clone();
            let config = self.config.clone();
            handles.push(thread::spawn(move || {
                Self::mining_thread(thread_id, running, config);
            }));
        }

        for handle in handles {
            let _ = handle.join();
        }
        println!("Minerador encerrado.");
    }

    fn mining_thread(thread_id: usize, running: Arc<AtomicBool>, config: MinerConfig) {
        // Inicializa RandomX (modo rápido, false = não usar large pages)
        let randomx = match RandomX::new(false) {
            Ok(rx) => rx,
            Err(e) => {
                eprintln!("Thread {}: falha ao inicializar RandomX: {}", thread_id, e);
                return;
            }
        };

        // Conecta à pool (cada thread tem sua própria conexão)
        let mut client = match StratumClient::connect(&config.pool_url, &config.wallet) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Thread {}: erro ao conectar à pool: {}", thread_id, e);
                return;
            }
        };

        let mut total_hashes = 0u64;
        let start_total = Instant::now();
        let mut last_report = Instant::now();
        let mut last_hashes = 0u64;

        while running.load(Ordering::SeqCst) {
            // Obtém novo job
            let (job_id, blob_hex, target_hex) = match client.get_job() {
                Ok(job) => job,
                Err(e) => {
                    eprintln!("Thread {}: erro ao obter job: {}", thread_id, e);
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };
            let blob = hex::decode(blob_hex).expect("blob inválido");
            let target = hex::decode(target_hex).expect("target inválido");
            let target_bytes: [u8; 32] = {
                let mut arr = [0u8; 32];
                let len = target.len().min(32);
                arr[..len].copy_from_slice(&target[..len]);
                arr
            };

            // Loop de nonces com throttling
            let cycle_start = Instant::now();
            let mut nonce: u32 = 0;
            while cycle_start.elapsed().as_millis() < config.work_ms as u128 {
                // Prepara blob com nonce (posição 39)
                let mut blob_with_nonce = blob.clone();
                let nonce_bytes = nonce.to_le_bytes();
                blob_with_nonce[39..43].copy_from_slice(&nonce_bytes);
                // Calcula hash
                let hash = randomx.calculate_hash(&blob_with_nonce);
                total_hashes += 1;
                // Verifica dificuldade (se hash < target)
                if hash < target_bytes {
                    println!("Thread {}: share encontrado! nonce={}, hash={:?}", thread_id, nonce, hash);
                    let _ = client.submit(&job_id, nonce, &hash);
                }
                nonce = nonce.wrapping_add(1);
                if nonce == 0 {
                    break; // nonce exaurido, pega novo job
                }
            }
            // Pausa
            thread::sleep(Duration::from_millis(config.idle_ms));

            // Relatório periódico
            let now = Instant::now();
            if now.duration_since(last_report).as_secs_f64() >= 1.0 {
                let hashes_in_period = total_hashes - last_hashes;
                let elapsed = now.duration_since(last_report).as_secs_f64();
                let hashrate = hashes_in_period as f64 / elapsed;
                println!("Thread {}: {:.2} H/s (total {} hashes)", thread_id, hashrate, total_hashes);
                last_report = now;
                last_hashes = total_hashes;
            }
        }
        let total_elapsed = start_total.elapsed().as_secs_f64();
        let avg_hashrate = total_hashes as f64 / total_elapsed;
        println!("Thread {} finalizada. Média: {:.2} H/s, total hashes: {}", thread_id, avg_hashrate, total_hashes);
    }
}

// ---------- Main ----------
fn main() {
    let args = Args::parse();
    println!("=== Minerador Monero (RandomX) ===");
    println!("Pool: {}", args.pool);
    println!("Carteira: {}", args.wallet);
    println!("CPU alvo: {}%", args.cpu_percent);

    let miner = Miner::new(args.wallet, args.pool, args.cpu_percent);
    miner.start();
}
