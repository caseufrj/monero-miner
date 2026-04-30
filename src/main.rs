use std::fs;
use std::io::{self, Write};
use std::process::{Command, Child};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use serde::{Serialize, Deserialize};

// Configuração salva em arquivo
#[derive(Serialize, Deserialize, Clone)]
struct Config {
    pool_url: String,
    wallet: String,
    cpu_percent: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pool_url: "pool.supportxmr.com:5555".into(),
            wallet: String::new(),
            cpu_percent: 50,
        }
    }
}

// Gerenciador do processo xmrig
struct XmrigWrapper {
    process: Option<Child>,
    suspended: bool,
    running: Arc<AtomicBool>,
}

impl XmrigWrapper {
    fn new() -> Self {
        Self {
            process: None,
            suspended: false,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    // Inicia o xmrig com base nas configurações
    fn start(&mut self, config: &Config) -> Result<(), String> {
        let total_cores = num_cpus::get();
        let num_threads = ((total_cores as f32 * config.cpu_percent as f32 / 100.0).round() as usize).max(1);
        println!("Iniciando xmrig com {} threads ({}% de {} núcleos)", num_threads, config.cpu_percent, total_cores);
        
        // Caminho do xmrig.exe (espera-se que esteja na mesma pasta do wrapper)
        let xmrig_path = "xmrig.exe";
        if !std::path::Path::new(xmrig_path).exists() {
            return Err("xmrig.exe não encontrado. Coloque-o na mesma pasta do wrapper.".into());
        }

        let mut cmd = Command::new(xmrig_path);
        cmd.arg("--url").arg(&config.pool_url)
           .arg("--user").arg(&config.wallet)
           .arg("--threads").arg(num_threads.to_string())
           .arg("--no-color")
           .arg("--background"); // roda em segundo plano (sem janela)
        // Opcional: desabilitar TLS se necessário (--tls false)
        
        match cmd.spawn() {
            Ok(child) => {
                self.process = Some(child);
                self.suspended = false;
                Ok(())
            },
            Err(e) => Err(format!("Erro ao iniciar xmrig: {}", e)),
        }
    }

    // Pausa o processo (suspende todas as threads)
    fn suspend(&mut self) -> Result<(), String> {
        if let Some(ref mut child) = self.process {
            // Obtém o handle do processo
            use winapi::um::processthreadsapi::{OpenProcess, SuspendThread, Thread32First, Thread32Next};
            use winapi::um::tlhelp32::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, PROCESSENTRY32, Thread32First, Thread32Next};
            use winapi::um::handleapi::CloseHandle;
            use winapi::shared::minwindef::DWORD;
            use winapi::um::winnt::PROCESS_SUSPEND_RESUME;

            let pid = child.id();
            unsafe {
                let h_process = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
                if h_process.is_null() {
                    return Err("Não foi possível abrir o processo".into());
                }
                // Snapshot das threads
                let h_snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
                if h_snap as isize == -1_i64 {
                    CloseHandle(h_process);
                    return Err("Falha ao criar snapshot".into());
                }
                let mut te: winapi::um::tlhelp32::THREADENTRY32 = std::mem::zeroed();
                te.dwSize = std::mem::size_of::<winapi::um::tlhelp32::THREADENTRY32>() as DWORD;
                if Thread32First(h_snap, &mut te) == 1 {
                    loop {
                        if te.th32OwnerProcessID == pid {
                            let h_thread = OpenProcess(PROCESS_SUSPEND_RESUME, 0, te.th32ThreadID);
                            if !h_thread.is_null() {
                                SuspendThread(h_thread);
                                CloseHandle(h_thread);
                            }
                        }
                        if Thread32Next(h_snap, &mut te) != 1 {
                            break;
                        }
                    }
                }
                CloseHandle(h_snap);
                CloseHandle(h_process);
            }
            self.suspended = true;
            println!("⚙️  Mineração pausada.");
            Ok(())
        } else {
            Err("Processo não está rodando".into())
        }
    }

    // Retoma o processo
    fn resume(&mut self) -> Result<(), String> {
        if let Some(ref mut child) = self.process {
            use winapi::um::processthreadsapi::{OpenProcess, ResumeThread};
            use winapi::um::tlhelp32::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, Thread32First, Thread32Next};
            use winapi::um::handleapi::CloseHandle;
            use winapi::shared::minwindef::DWORD;
            use winapi::um::winnt::PROCESS_SUSPEND_RESUME;

            let pid = child.id();
            unsafe {
                let h_process = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
                if h_process.is_null() {
                    return Err("Não foi possível abrir o processo".into());
                }
                let h_snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
                if h_snap as isize == -1_i64 {
                    CloseHandle(h_process);
                    return Err("Falha ao criar snapshot".into());
                }
                let mut te: winapi::um::tlhelp32::THREADENTRY32 = std::mem::zeroed();
                te.dwSize = std::mem::size_of::<winapi::um::tlhelp32::THREADENTRY32>() as DWORD;
                if Thread32First(h_snap, &mut te) == 1 {
                    loop {
                        if te.th32OwnerProcessID == pid {
                            let h_thread = OpenProcess(PROCESS_SUSPEND_RESUME, 0, te.th32ThreadID);
                            if !h_thread.is_null() {
                                ResumeThread(h_thread);
                                CloseHandle(h_thread);
                            }
                        }
                        if Thread32Next(h_snap, &mut te) != 1 {
                            break;
                        }
                    }
                }
                CloseHandle(h_snap);
                CloseHandle(h_process);
            }
            self.suspended = false;
            println!("▶️  Mineração retomada.");
            Ok(())
        } else {
            Err("Processo não está rodando".into())
        }
    }

    // Para o processo
    fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
            println!("✅ xmrig finalizado.");
        }
        self.suspended = false;
        self.running.store(false, Ordering::SeqCst);
    }
}

// Carrega ou cria configuração interativa
fn load_or_create_config() -> Config {
    let config_path = "miner_config.toml";
    if let Ok(contents) = fs::read_to_string(config_path) {
        if let Ok(config) = toml::from_str(&contents) {
            println!("✅ Configuração carregada de '{}'", config_path);
            return config;
        }
    }

    println!("⚙️  Configuração inicial - digite os dados:");
    let mut config = Config::default();

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

    print!("🖥️  Percentual de CPU (10-100) [{}]: ", config.cpu_percent);
    io::stdout().flush().unwrap();
    input.clear();
    io::stdin().read_line(&mut input).unwrap();
    if !input.trim().is_empty() {
        let percent: u8 = input.trim().parse().expect("Número entre 10 e 100");
        config.cpu_percent = percent.clamp(10, 100);
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

// Função de throttling: suspende/resume ciclicamente
fn throttling_loop(wrapper: Arc<std::sync::Mutex<XmrigWrapper>>, work_ms: u64, idle_ms: u64, running: Arc<AtomicBool>) {
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(work_ms));
        // suspende
        {
            let mut w = wrapper.lock().unwrap();
            if w.process.is_some() && !w.suspended {
                let _ = w.suspend();
            }
        }
        thread::sleep(Duration::from_millis(idle_ms));
        // retoma
        {
            let mut w = wrapper.lock().unwrap();
            if w.process.is_some() && w.suspended {
                let _ = w.resume();
            }
        }
    }
}

fn main() {
    println!("=== XMRig Wrapper com controle de CPU ===\n");
    let config = load_or_create_config();
    let total_cores = num_cpus::get();
    let work_ms = 500; // meio segundo trabalhando
    let idle_ms = if config.cpu_percent == 100 {
        0
    } else {
        let percent = config.cpu_percent as f32;
        (work_ms as f32 * (100.0 - percent) / percent).round() as u64
    };
    println!("✅ Configuração OK. Iniciando mineração com {}% da CPU...", config.cpu_percent);

    let mut wrapper = XmrigWrapper::new();
    if let Err(e) = wrapper.start(&config) {
        eprintln!("Erro: {}", e);
        return;
    }

    let wrapper = Arc::new(std::sync::Mutex::new(wrapper));
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let w = wrapper.clone();

    // Configura Ctrl+C
    ctrlc::set_handler(move || {
        println!("\n🛑 Encerrando wrapper...");
        r.store(false, Ordering::SeqCst);
        let mut w = w.lock().unwrap();
        w.stop();
    }).expect("Erro ao configurar Ctrl+C");

    // Se cpu_percent < 100, inicia thread de throttling
    let throttling_handle = if config.cpu_percent < 100 {
        let w = wrapper.clone();
        let r = running.clone();
        Some(thread::spawn(move || {
            throttling_loop(w, work_ms, idle_ms, r);
        }))
    } else {
        None
    };

    // Aguarda sinal de parada
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(500));
        // Check se o processo xmrig morreu sozinho
        let process_exists = {
            let w = wrapper.lock().unwrap();
            w.process.is_some()
        };
        if !process_exists {
            println!("⚠️ xmrig foi encerrado inesperadamente. Saindo.");
            running.store(false, Ordering::SeqCst);
        }
    }

    if let Some(handle) = throttling_handle {
        let _ = handle.join();
    }
    println!("Wrapper finalizado.");
}
