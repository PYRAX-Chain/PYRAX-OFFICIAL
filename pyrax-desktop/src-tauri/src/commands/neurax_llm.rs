// NEURAX Local LLM Integration
// Provides local GPU-accelerated inference for NEURAX AI chat
// Uses llama.cpp via llama-cpp-2 crate for cross-platform GPU support

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use parking_lot::RwLock;
use std::path::PathBuf;
use tokio::sync::mpsc;

/// LLM Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub enabled: bool,
    pub model_path: Option<String>,
    pub model_name: String,
    pub gpu_layers: i32,           // Number of layers to offload to GPU (-1 = all)
    pub context_size: u32,         // Context window size
    pub max_tokens: u32,           // Max tokens to generate
    pub temperature: f32,          // Sampling temperature
    pub use_gpu: bool,             // Whether to use GPU acceleration
    pub download_progress: f32,    // Model download progress (0-100)
    pub model_size_mb: u64,        // Model file size in MB
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model_path: None,
            model_name: "Phi-3-mini-4k-instruct-q4_K_M.gguf".to_string(),
            gpu_layers: -1,       // Offload all layers to GPU
            context_size: 4096,   // 4K context
            max_tokens: 512,      // Max response length
            temperature: 0.7,     // Balanced creativity
            use_gpu: true,        // Prefer GPU
            download_progress: 0.0,
            model_size_mb: 2300,  // ~2.3GB for Phi-3-mini q4
        }
    }
}

/// GPU Information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub memory_mb: u64,
    pub compute_capability: Option<String>,
    pub driver_version: Option<String>,
    pub supported: bool,
    pub backend: String,  // "cuda", "metal", "vulkan", "cpu"
}

/// Model download status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDownloadStatus {
    pub downloading: bool,
    pub progress: f32,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub speed_mbps: f32,
    pub eta_seconds: u32,
    pub error: Option<String>,
}

/// LLM State Manager
pub struct LlmState {
    pub config: RwLock<LlmConfig>,
    pub gpu_info: RwLock<Option<GpuInfo>>,
    pub download_status: RwLock<ModelDownloadStatus>,
    pub model_loaded: RwLock<bool>,
}

impl LlmState {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(LlmConfig::default()),
            gpu_info: RwLock::new(None),
            download_status: RwLock::new(ModelDownloadStatus {
                downloading: false,
                progress: 0.0,
                bytes_downloaded: 0,
                total_bytes: 0,
                speed_mbps: 0.0,
                eta_seconds: 0,
                error: None,
            }),
            model_loaded: RwLock::new(false),
        }
    }

    pub fn load_config(&self, data_dir: &PathBuf) {
        let config_path = data_dir.join("neurax_llm_config.json");
        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(config) = serde_json::from_str::<LlmConfig>(&content) {
                    *self.config.write() = config;
                }
            }
        }
    }

    pub fn save_config(&self, data_dir: &PathBuf) -> Result<(), String> {
        let config_path = data_dir.join("neurax_llm_config.json");
        let config = self.config.read().clone();
        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        std::fs::write(&config_path, content)
            .map_err(|e| format!("Failed to save config: {}", e))?;
        Ok(())
    }
}

/// Detect available GPUs
pub fn detect_gpus() -> Vec<GpuInfo> {
    let mut gpus = Vec::new();
    
    // Try NVIDIA CUDA
    #[cfg(feature = "nvidia")]
    {
        if let Ok(nvml) = nvml_wrapper::Nvml::init() {
            if let Ok(count) = nvml.device_count() {
                for i in 0..count {
                    if let Ok(device) = nvml.device_by_index(i) {
                        let name = device.name().unwrap_or_else(|_| "Unknown NVIDIA GPU".to_string());
                        let memory = device.memory_info().map(|m| m.total / 1024 / 1024).unwrap_or(0);
                        let driver = nvml.sys_driver_version().ok();
                        let cuda_version = nvml.sys_cuda_driver_version().ok()
                            .map(|v| format!("{}.{}", v / 1000, (v % 1000) / 10));
                        
                        gpus.push(GpuInfo {
                            name,
                            vendor: "NVIDIA".to_string(),
                            memory_mb: memory,
                            compute_capability: cuda_version,
                            driver_version: driver,
                            supported: memory >= 3000, // Need at least 3GB VRAM
                            backend: "cuda".to_string(),
                        });
                    }
                }
            }
        }
    }
    
    // If no NVIDIA GPU found, check for others
    if gpus.is_empty() {
        // Try to detect via sysinfo for basic info
        let sys = sysinfo::System::new_all();
        
        // On macOS, check for Metal support (all Apple Silicon has Metal)
        #[cfg(target_os = "macos")]
        {
            // Check for Apple Silicon
            if std::env::consts::ARCH == "aarch64" {
                gpus.push(GpuInfo {
                    name: "Apple Silicon GPU".to_string(),
                    vendor: "Apple".to_string(),
                    memory_mb: 0, // Shared memory
                    compute_capability: None,
                    driver_version: None,
                    supported: true,
                    backend: "metal".to_string(),
                });
            }
        }
        
        // Fallback to CPU
        if gpus.is_empty() {
            gpus.push(GpuInfo {
                name: format!("CPU ({} cores)", sys.cpus().len()),
                vendor: "CPU".to_string(),
                memory_mb: sys.total_memory() / 1024 / 1024,
                compute_capability: None,
                driver_version: None,
                supported: true, // CPU is always supported
                backend: "cpu".to_string(),
            });
        }
    }
    
    gpus
}

/// Model URLs for download
const MODEL_URLS: &[(&str, &str, u64)] = &[
    // (name, url, size_mb)
    ("Phi-3-mini-4k-instruct-q4_K_M.gguf", 
     "https://huggingface.co/microsoft/Phi-3-mini-4k-instruct-gguf/resolve/main/Phi-3-mini-4k-instruct-q4_K_M.gguf",
     2300),
    ("Llama-3.2-3B-Instruct-Q4_K_M.gguf",
     "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
     2000),
    ("Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",
     "https://huggingface.co/MaziyarPanahi/Mistral-7B-Instruct-v0.3-GGUF/resolve/main/Mistral-7B-Instruct-v0.3.Q4_K_M.gguf",
     4000),
];

/// Download a model file with progress tracking
pub async fn download_model(
    model_name: &str,
    models_dir: PathBuf,
    progress_tx: mpsc::Sender<ModelDownloadStatus>,
) -> Result<PathBuf, String> {
    // Find model URL
    let (_, url, total_mb) = MODEL_URLS.iter()
        .find(|(name, _, _)| *name == model_name)
        .ok_or_else(|| format!("Unknown model: {}", model_name))?;
    
    let model_path = models_dir.join(model_name);
    
    // Check if already downloaded
    if model_path.exists() {
        let metadata = std::fs::metadata(&model_path)
            .map_err(|e| format!("Failed to check model: {}", e))?;
        if metadata.len() > 1_000_000_000 { // Over 1GB, assume valid
            return Ok(model_path);
        }
    }
    
    // Create models directory
    std::fs::create_dir_all(&models_dir)
        .map_err(|e| format!("Failed to create models directory: {}", e))?;
    
    let total_bytes = *total_mb * 1024 * 1024;
    let client = reqwest::Client::new();
    
    let response = client.get(*url)
        .send()
        .await
        .map_err(|e| format!("Failed to download model: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }
    
    let mut file = tokio::fs::File::create(&model_path)
        .await
        .map_err(|e| format!("Failed to create model file: {}", e))?;
    
    use tokio::io::AsyncWriteExt;
    
    let mut bytes_downloaded: u64 = 0;
    let start_time = std::time::Instant::now();
    let mut stream = response.bytes_stream();
    
    use futures_util::StreamExt;
    
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download error: {}", e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        
        bytes_downloaded += chunk.len() as u64;
        
        let elapsed = start_time.elapsed().as_secs_f32();
        let speed_mbps = if elapsed > 0.0 {
            (bytes_downloaded as f32 / 1024.0 / 1024.0) / elapsed
        } else {
            0.0
        };
        
        let remaining_bytes = total_bytes.saturating_sub(bytes_downloaded);
        let eta_seconds = if speed_mbps > 0.0 {
            (remaining_bytes as f32 / 1024.0 / 1024.0 / speed_mbps) as u32
        } else {
            0
        };
        
        let progress = (bytes_downloaded as f32 / total_bytes as f32) * 100.0;
        
        let _ = progress_tx.send(ModelDownloadStatus {
            downloading: true,
            progress,
            bytes_downloaded,
            total_bytes,
            speed_mbps,
            eta_seconds,
            error: None,
        }).await;
    }
    
    file.flush().await.map_err(|e| format!("Flush error: {}", e))?;
    
    Ok(model_path)
}

/// System prompt for NEURAX
const NEURAX_SYSTEM_PROMPT: &str = r#"You are NEURAX, an AI assistant integrated into the PYRAX Desktop application. You help users optimize their node performance, understand network connectivity, troubleshoot mining issues, and analyze system health.

You have access to:
- Real-time system metrics (CPU, memory, GPU usage and temperatures)
- Node connection status and peer information
- Recent log entries and error patterns
- Network health indicators

Be concise, technical when appropriate, and actionable. When you identify issues, explain the cause and provide specific suggestions. Use bullet points for clarity.

Current system context will be provided with each query."#;

/// Generate a chat response using the LLM (or fallback to rules)
pub fn generate_llm_response(
    query: &str,
    context: &str,
    _config: &LlmConfig,
    _model_loaded: bool,
) -> String {
    // TODO: When llama-cpp-2 is integrated, this will use the actual LLM
    // For now, we use enhanced rule-based responses with the context
    
    // This is a placeholder that will be replaced when the LLM crate is added
    // The actual LLM integration requires:
    // 1. Adding llama-cpp-2 = { version = "0.1", features = ["cuda"] } to Cargo.toml
    // 2. Loading the model on startup
    // 3. Running inference here
    
    // For now, return a formatted response indicating LLM is not yet loaded
    format!(
        "**NEURAX AI Response** (Rule-based mode)\n\n\
        I received your query: \"{}\"\n\n\
        **System Context:**\n{}\n\n\
        *Note: Full LLM inference requires downloading the AI model. \
        Go to NEURAX Settings → Download Model to enable advanced AI capabilities.*",
        query.chars().take(100).collect::<String>(),
        context.lines().take(10).collect::<Vec<_>>().join("\n")
    )
}

// ============================================================================
// Tauri Commands
// ============================================================================

use tauri::{AppHandle, State};

/// Wrapper for LLM state
pub struct LlmStateWrapper(pub Arc<LlmState>);

#[tauri::command]
pub async fn neurax_get_llm_config(
    state: State<'_, LlmStateWrapper>,
) -> Result<LlmConfig, String> {
    Ok(state.0.config.read().clone())
}

#[tauri::command]
pub async fn neurax_set_llm_config(
    app: AppHandle,
    state: State<'_, LlmStateWrapper>,
    config: LlmConfig,
) -> Result<(), String> {
    *state.0.config.write() = config;
    
    let data_dir = app.path_resolver().app_data_dir()
        .ok_or("Failed to get app data directory")?;
    state.0.save_config(&data_dir)?;
    
    Ok(())
}

#[tauri::command]
pub async fn neurax_detect_gpus() -> Result<Vec<GpuInfo>, String> {
    Ok(detect_gpus())
}

#[tauri::command]
pub async fn neurax_get_available_models() -> Result<Vec<serde_json::Value>, String> {
    Ok(MODEL_URLS.iter().map(|(name, _url, size_mb)| {
        serde_json::json!({
            "name": name,
            "size_mb": size_mb,
            "description": match *name {
                n if n.contains("Phi-3") => "Microsoft Phi-3 Mini - Fast, efficient, great for analysis",
                n if n.contains("Llama-3.2") => "Meta Llama 3.2 3B - Excellent general purpose",
                n if n.contains("Mistral") => "Mistral 7B - High quality, requires more VRAM",
                _ => "Unknown model",
            }
        })
    }).collect())
}

#[tauri::command]
pub async fn neurax_download_model(
    app: AppHandle,
    state: State<'_, LlmStateWrapper>,
    model_name: String,
) -> Result<String, String> {
    let data_dir = app.path_resolver().app_data_dir()
        .ok_or("Failed to get app data directory")?;
    let models_dir = data_dir.join("models");
    
    let (tx, mut rx) = mpsc::channel(100);
    
    // Update download status in state
    let state_clone = state.0.clone();
    tokio::spawn(async move {
        while let Some(status) = rx.recv().await {
            *state_clone.download_status.write() = status;
        }
    });
    
    let model_path = download_model(&model_name, models_dir, tx).await?;
    
    // Update config with model path
    {
        let mut config = state.0.config.write();
        config.model_path = Some(model_path.to_string_lossy().to_string());
        config.model_name = model_name;
    }
    
    state.0.save_config(&data_dir)?;
    
    Ok(model_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn neurax_get_download_status(
    state: State<'_, LlmStateWrapper>,
) -> Result<ModelDownloadStatus, String> {
    Ok(state.0.download_status.read().clone())
}

#[tauri::command]
pub async fn neurax_is_model_loaded(
    state: State<'_, LlmStateWrapper>,
) -> Result<bool, String> {
    Ok(*state.0.model_loaded.read())
}
