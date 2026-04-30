use std::fs;
use std::io::{self, Write};
use std::process::{Command, Child};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use serde::{Deserialize, Serialize};

// ---------- Configuração ----------
#[derive(Serialize, Deserialize, Clone)]
struct MinerConfig {
    pool_url: String,
    wallet: String,
    threads: usize,    // número de threads do XMRig (controla % CPU)
}

impl Default for MinerConfig {
    fn default() -> Self {
        Self {
            pool_url: "pool.supportxmr.com:5555".into(),
            wallet: String::new(),
            threads: 2, // padrão 50% num quad-core
        }
    }
}

// ---------- Helper: carregar/salvar config ----------
fn load_or_create_config() -> MinerConfig {
    let config_path = "miner_config.toml";
    if let Ok(contents) = fs::read_to_string(config_path) {
        if let Ok(config) = toml::from_str(&contents) {
            println!("✅ Configuração carregada de '{}'", config_path);
            return config;
        }
    }
    println!("⚙️  Configuração inicial – responda as perguntas abaixo.");
    let mut config = MinerConfig::default();

    print!("🌐 Pool (ex: pool.supportxmr.com:5555) [{}]: ", config.pool_url);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    if !input.trim().is_empty() {
        config.pool_url = input.trim().to_string();
    }

    print!("💰 Endereço da carteira Monero: ");
    io::stdout().flush().unwrap();
    input.clear();
    io::stdin().read_line(&mut input).unwrap();
    let wallet = input.trim().to_string();
    if wallet.is_empty() {
        panic!("Carteira é obrigatória!");
    }
    config.wallet = wallet;

    print!("🖥️  Número de threads (1 a 4) [2]: ");
    io::stdout().flush().unwrap();
    input.clear();
    io::stdin().read_line(&mut input).unwrap();
    if !input.trim().is_empty() {
        let t = input.trim().parse().expect("Número");
        config.threads = t.clamp(1, 4);
    }

    print!("💾 Salvar configuração para uso futuro? (s/N): ");
    io::stdout().flush().unwrap();
    input.clear();
    io::stdin().read_line(&mut input).unwrap();
    if input.trim().to_lowercase() == "s" {
        let toml = toml::to_string(&config).unwrap();
        fs::write(config_path, toml).unwrap();
        println!("✅ Configuração salva em '{}'", config_path);
    }
    config
}

// ---------- Wrapper do XMRig ----------
struct XmrigWrapper {
    child: Option<Child>,
    running: Arc<AtomicBool>,
}

impl XmrigWrapper {
    fn new() -> Self {
        Self {
            child: None,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    fn start(&mut self, config: &MinerConfig) -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::new("xmrig.exe");
        cmd.arg("--url").arg(&config.pool_url)
           .arg("--user").arg(&config.wallet)
           .arg("--threads").arg(config.threads.to_string())
           .arg("--no-color")
           .arg("--background")   // roda em segundo plano mas ainda vemos logs?
           .arg("--log-file").arg("xmrig.log"); // opcional
        // Para visualizar logs no console, remova --background e redirecione stdout
        // Aqui vamos rodar sem background para ver os logs no mesmo terminal
        let mut child = cmd.spawn()?;
        self.child = Some(child);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// ---------- Main ----------
fn main() {
    println!("=== Minerador Monero (Wrapper XMRig) ===\n");
    let config = load_or_create_config();

    println!("\n📊 Configuração atual:");
    println!("   Pool: {}", config.pool_url);
    println!("   Carteira: {}", config.wallet);
    println!("   Threads: {}", config.threads);
    println!("\n🔧 Iniciando XMRig...\n");

    let mut wrapper = XmrigWrapper::new();
    if let Err(e) = wrapper.start(&config) {
        eprintln!("Erro ao iniciar XMRig: {}", e);
        eprintln!("Certifique-se que o arquivo 'xmrig.exe' está na mesma pasta.");
        return;
    }

    // Configura Ctrl+C para encerrar o XMRig
    let running = wrapper.running.clone();
    ctrlc::set_handler(move || {
        println!("\n🛑 Encerrando minerador...");
        running.store(false, Ordering::SeqCst);
    }).expect("Erro ao configurar Ctrl+C");

    // Aguarda até que o usuário peça para sair
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(1));
    }

    wrapper.stop();
    println!("Minerador finalizado.");
}
