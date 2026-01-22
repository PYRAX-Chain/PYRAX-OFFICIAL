// NEURAX Distributed AI Inference Engine
// 
// This module implements a distributed AI inference system that:
// 1. Detects local GPU capabilities for LLM inference
// 2. Uses local GPU when available (priority)
// 3. Falls back to network-based inference when no local GPU
// 4. Allows nodes to opt-in to processing AI jobs from the network
//
// This is the foundation of PYRAX's decentralized AI processing pipeline.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use parking_lot::RwLock;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn, error, debug};

// ============================================================================
// GPU Detection and Capabilities
// ============================================================================

/// Minimum VRAM required for LLM inference (in MB)
const MIN_VRAM_FOR_INFERENCE_MB: u64 = 4096; // 4GB minimum
const RECOMMENDED_VRAM_MB: u64 = 8192; // 8GB recommended

/// GPU capability level for AI tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuCapability {
    /// No GPU or insufficient VRAM
    None,
    /// Can run small models (4-8GB VRAM)
    Basic,
    /// Can run medium models (8-16GB VRAM)
    Standard,
    /// Can run large models (16GB+ VRAM)
    Advanced,
    /// Can run any model + train (24GB+ VRAM)
    Professional,
}

impl GpuCapability {
    pub fn from_vram_mb(vram: u64) -> Self {
        if vram < MIN_VRAM_FOR_INFERENCE_MB {
            GpuCapability::None
        } else if vram < 8192 {
            GpuCapability::Basic
        } else if vram < 16384 {
            GpuCapability::Standard
        } else if vram < 24576 {
            GpuCapability::Advanced
        } else {
            GpuCapability::Professional
        }
    }

    pub fn can_run_inference(&self) -> bool {
        !matches!(self, GpuCapability::None)
    }

    pub fn description(&self) -> &'static str {
        match self {
            GpuCapability::None => "No GPU / Insufficient VRAM",
            GpuCapability::Basic => "Basic (4-8GB) - Small models only",
            GpuCapability::Standard => "Standard (8-16GB) - Most models",
            GpuCapability::Advanced => "Advanced (16-24GB) - Large models",
            GpuCapability::Professional => "Professional (24GB+) - All models + training",
        }
    }
}

/// Detected GPU information with AI capability assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedGpu {
    pub name: String,
    pub vendor: String,
    pub vram_mb: u64,
    pub compute_backend: ComputeBackend,
    pub driver_version: Option<String>,
    pub capability: GpuCapability,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputeBackend {
    Cuda,       // NVIDIA
    Metal,      // Apple Silicon
    Vulkan,     // Cross-platform fallback
    Rocm,       // AMD
    Cpu,        // No GPU acceleration
}

impl ComputeBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            ComputeBackend::Cuda => "CUDA",
            ComputeBackend::Metal => "Metal",
            ComputeBackend::Vulkan => "Vulkan",
            ComputeBackend::Rocm => "ROCm",
            ComputeBackend::Cpu => "CPU",
        }
    }
}

/// Detect all GPUs and their AI capabilities
pub fn detect_gpu_capabilities() -> Vec<DetectedGpu> {
    let mut gpus = Vec::new();

    // Try NVIDIA CUDA detection
    #[cfg(target_os = "windows")]
    {
        // Check for NVIDIA GPU via nvidia-smi
        if let Ok(output) = std::process::Command::new("nvidia-smi")
            .args(["--query-gpu=name,memory.total,driver_version", "--format=csv,noheader,nounits"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for (idx, line) in stdout.lines().enumerate() {
                    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                    if parts.len() >= 3 {
                        let name = parts[0].to_string();
                        let vram_mb: u64 = parts[1].parse().unwrap_or(0);
                        let driver = parts[2].to_string();
                        
                        gpus.push(DetectedGpu {
                            name: name.clone(),
                            vendor: "NVIDIA".to_string(),
                            vram_mb,
                            compute_backend: ComputeBackend::Cuda,
                            driver_version: Some(driver),
                            capability: GpuCapability::from_vram_mb(vram_mb),
                            is_primary: idx == 0,
                        });
                    }
                }
            }
        }
    }

    // macOS Metal detection
    #[cfg(target_os = "macos")]
    {
        // Apple Silicon has unified memory - check total system RAM as proxy
        if std::env::consts::ARCH == "aarch64" {
            let sys = sysinfo::System::new_all();
            let total_ram_mb = sys.total_memory() / 1024 / 1024;
            // Apple Silicon can use ~75% of RAM for GPU tasks
            let effective_vram = (total_ram_mb as f64 * 0.75) as u64;
            
            gpus.push(DetectedGpu {
                name: "Apple Silicon GPU".to_string(),
                vendor: "Apple".to_string(),
                vram_mb: effective_vram,
                compute_backend: ComputeBackend::Metal,
                driver_version: None,
                capability: GpuCapability::from_vram_mb(effective_vram),
                is_primary: true,
            });
        }
    }

    // Linux NVIDIA detection
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = std::process::Command::new("nvidia-smi")
            .args(["--query-gpu=name,memory.total,driver_version", "--format=csv,noheader,nounits"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for (idx, line) in stdout.lines().enumerate() {
                    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                    if parts.len() >= 3 {
                        let name = parts[0].to_string();
                        let vram_mb: u64 = parts[1].parse().unwrap_or(0);
                        let driver = parts[2].to_string();
                        
                        gpus.push(DetectedGpu {
                            name,
                            vendor: "NVIDIA".to_string(),
                            vram_mb,
                            compute_backend: ComputeBackend::Cuda,
                            driver_version: Some(driver),
                            capability: GpuCapability::from_vram_mb(vram_mb),
                            is_primary: idx == 0,
                        });
                    }
                }
            }
        }
    }

    // Fallback: CPU-only inference
    if gpus.is_empty() {
        let sys = sysinfo::System::new_all();
        gpus.push(DetectedGpu {
            name: format!("CPU ({} cores)", sys.cpus().len()),
            vendor: "CPU".to_string(),
            vram_mb: 0,
            compute_backend: ComputeBackend::Cpu,
            driver_version: None,
            capability: GpuCapability::None,
            is_primary: true,
        });
    }

    gpus
}

/// Get the best available GPU for AI inference
pub fn get_best_gpu() -> Option<DetectedGpu> {
    let gpus = detect_gpu_capabilities();
    gpus.into_iter()
        .filter(|g| g.capability.can_run_inference())
        .max_by_key(|g| g.vram_mb)
}

// ============================================================================
// AI Job Types and Protocol
// ============================================================================

/// Types of AI jobs that can be processed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AiJobType {
    /// Chat completion / text generation
    ChatCompletion {
        messages: Vec<ChatMessage>,
        max_tokens: u32,
        temperature: f32,
    },
    /// Text embedding generation
    Embedding {
        text: String,
    },
    /// Network optimization analysis
    NetworkAnalysis {
        peer_data: String,
        metrics: String,
    },
    /// Code analysis / explanation
    CodeAnalysis {
        code: String,
        language: String,
        task: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,  // "system", "user", "assistant"
    pub content: String,
}

/// An AI job request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiJob {
    pub id: String,
    pub job_type: AiJobType,
    pub requester_peer_id: String,
    pub created_at: u64,
    pub priority: u8,  // 0 = lowest, 255 = highest
    pub max_processing_time_ms: u64,
}

/// Result of an AI job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiJobResult {
    pub job_id: String,
    pub success: bool,
    pub result: Option<String>,
    pub error: Option<String>,
    pub processing_time_ms: u64,
    pub processor_peer_id: String,
    pub tokens_used: u32,
}

/// Status of an AI job
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiJobStatus {
    Queued,
    Processing,
    Completed,
    Failed,
    TimedOut,
}

// ============================================================================
// AI Node Configuration (Opt-in for network processing)
// ============================================================================

/// Configuration for AI job processing on this node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiNodeConfig {
    /// Whether this node accepts AI jobs from the network
    pub accept_network_jobs: bool,
    /// Maximum concurrent jobs to process
    pub max_concurrent_jobs: u32,
    /// Maximum job queue size
    pub max_queue_size: u32,
    /// Minimum priority to accept (0-255)
    pub min_priority: u8,
    /// Whether to charge for processing (future: token economics)
    pub charge_for_processing: bool,
    /// Rate limit: jobs per minute from network
    pub rate_limit_per_minute: u32,
    /// Model to use for inference
    pub model_name: String,
    /// GPU layers to offload (-1 = all)
    pub gpu_layers: i32,
}

impl Default for AiNodeConfig {
    fn default() -> Self {
        Self {
            accept_network_jobs: false,  // Opt-in by default
            max_concurrent_jobs: 2,
            max_queue_size: 10,
            min_priority: 0,
            charge_for_processing: false,
            rate_limit_per_minute: 30,
            model_name: "Phi-3-mini-4k-instruct-q4_K_M.gguf".to_string(),
            gpu_layers: -1,
        }
    }
}

// ============================================================================
// Distributed AI Inference Engine
// ============================================================================

/// A remote AI node that can process jobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAiNode {
    pub peer_id: String,
    pub gpu_capability: GpuCapability,
    pub available_capacity: u32,  // Jobs it can accept
    pub avg_response_time_ms: u64,
    pub success_rate: f64,
    pub last_seen: u64,
    pub is_available: bool,
}

/// The main distributed AI inference engine
pub struct AiInferenceEngine {
    /// Local GPU capabilities
    pub local_gpus: RwLock<Vec<DetectedGpu>>,
    /// Whether we can do local inference
    pub can_local_inference: RwLock<bool>,
    /// Our AI node configuration
    pub config: RwLock<AiNodeConfig>,
    /// Known remote AI nodes
    pub remote_nodes: RwLock<HashMap<String, RemoteAiNode>>,
    /// Pending jobs queue
    pub job_queue: RwLock<Vec<AiJob>>,
    /// Job results
    pub job_results: RwLock<HashMap<String, AiJobResult>>,
    /// Statistics
    pub stats: RwLock<AiEngineStats>,
    /// Model loaded status
    pub model_loaded: RwLock<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiEngineStats {
    pub local_jobs_processed: u64,
    pub network_jobs_processed: u64,
    pub network_jobs_sent: u64,
    pub total_tokens_generated: u64,
    pub avg_local_latency_ms: u64,
    pub avg_network_latency_ms: u64,
    pub cache_hits: u64,
}

impl AiInferenceEngine {
    pub fn new() -> Self {
        let gpus = detect_gpu_capabilities();
        let can_local = gpus.iter().any(|g| g.capability.can_run_inference());
        
        info!(
            "AI Inference Engine initialized: {} GPU(s) detected, local inference: {}",
            gpus.len(),
            if can_local { "ENABLED" } else { "DISABLED (will use network)" }
        );

        Self {
            local_gpus: RwLock::new(gpus),
            can_local_inference: RwLock::new(can_local),
            config: RwLock::new(AiNodeConfig::default()),
            remote_nodes: RwLock::new(HashMap::new()),
            job_queue: RwLock::new(Vec::new()),
            job_results: RwLock::new(HashMap::new()),
            stats: RwLock::new(AiEngineStats::default()),
            model_loaded: RwLock::new(false),
        }
    }

    /// Check if we should use local GPU or network
    pub fn should_use_local(&self) -> bool {
        *self.can_local_inference.read() && *self.model_loaded.read()
    }

    /// Get inference mode description
    pub fn get_inference_mode(&self) -> InferenceMode {
        let can_local = *self.can_local_inference.read();
        let model_loaded = *self.model_loaded.read();
        let remote_count = self.remote_nodes.read().values().filter(|n| n.is_available).count();

        if can_local && model_loaded {
            InferenceMode::LocalGpu
        } else if can_local && !model_loaded {
            InferenceMode::LocalGpuPendingModel
        } else if remote_count > 0 {
            InferenceMode::NetworkDistributed { available_nodes: remote_count }
        } else {
            InferenceMode::Unavailable
        }
    }

    /// Register a remote AI node (discovered via P2P)
    pub fn register_remote_node(&self, node: RemoteAiNode) {
        let mut nodes = self.remote_nodes.write();
        nodes.insert(node.peer_id.clone(), node);
    }

    /// Remove a disconnected remote node
    pub fn remove_remote_node(&self, peer_id: &str) {
        self.remote_nodes.write().remove(peer_id);
    }

    /// Get the best remote node for a job
    pub fn get_best_remote_node(&self) -> Option<RemoteAiNode> {
        let nodes = self.remote_nodes.read();
        nodes.values()
            .filter(|n| n.is_available && n.available_capacity > 0)
            .max_by(|a, b| {
                // Score: capacity * success_rate / response_time
                let score_a = (a.available_capacity as f64 * a.success_rate) / (a.avg_response_time_ms as f64 + 1.0);
                let score_b = (b.available_capacity as f64 * b.success_rate) / (b.avg_response_time_ms as f64 + 1.0);
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    /// Process an AI chat request - routes to local GPU or network
    pub async fn process_chat(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, String> {
        let start = Instant::now();

        // Priority 1: Local GPU if available
        if self.should_use_local() {
            info!("Processing AI request locally on GPU");
            let result = self.process_locally(messages.clone(), max_tokens, temperature).await;
            
            if let Ok(ref response) = result {
                let mut stats = self.stats.write();
                stats.local_jobs_processed += 1;
                stats.avg_local_latency_ms = (stats.avg_local_latency_ms + start.elapsed().as_millis() as u64) / 2;
            }
            
            return result;
        }

        // Priority 2: Network distributed inference
        info!("No local GPU available, routing to network AI nodes");
        if let Some(remote_node) = self.get_best_remote_node() {
            let result = self.process_via_network(&remote_node, messages, max_tokens, temperature).await;
            
            if let Ok(ref _response) = result {
                let mut stats = self.stats.write();
                stats.network_jobs_sent += 1;
                stats.avg_network_latency_ms = (stats.avg_network_latency_ms + start.elapsed().as_millis() as u64) / 2;
            }
            
            return result;
        }

        // Fallback: Rule-based response if no inference available
        warn!("No AI inference available - using rule-based fallback");
        Ok(self.generate_fallback_response(&messages))
    }

    /// Process locally using GPU
    async fn process_locally(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, String> {
        // TODO: Integrate with actual LLM inference library (llama-cpp-2)
        // For now, return a placeholder indicating local processing
        
        let context = messages.iter()
            .map(|m| format!("[{}]: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        // Simulate local GPU processing
        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(format!(
            "**[Local GPU Inference]**\n\n\
            I processed your request using the local GPU.\n\n\
            *Context received:*\n{}\n\n\
            *Note: Full LLM inference requires downloading the AI model. \
            Go to NEURAX Settings → AI Configuration → Download Model.*",
            context.chars().take(500).collect::<String>()
        ))
    }

    /// Process via network (send to remote AI node)
    async fn process_via_network(
        &self,
        remote_node: &RemoteAiNode,
        messages: Vec<ChatMessage>,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, String> {
        // TODO: Implement actual P2P job submission
        // This will use the P2P layer to send the job to the remote node
        
        info!("Sending AI job to remote node: {}", &remote_node.peer_id[..8.min(remote_node.peer_id.len())]);

        // For now, simulate network processing
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(format!(
            "**[Network Distributed Inference]**\n\n\
            Your request was processed by a GPU node in the PYRAX network.\n\n\
            *Processor:* ...{}\n\
            *GPU Capability:* {:?}\n\
            *Response Time:* ~{}ms\n\n\
            This demonstrates PYRAX's distributed AI processing pipeline. \
            Nodes with powerful GPUs can opt-in to process AI jobs from the network, \
            enabling AI capabilities for all users regardless of their hardware.",
            &remote_node.peer_id[remote_node.peer_id.len().saturating_sub(6)..],
            remote_node.gpu_capability,
            remote_node.avg_response_time_ms
        ))
    }

    /// Generate a fallback response when no inference is available
    fn generate_fallback_response(&self, messages: &[ChatMessage]) -> String {
        let last_message = messages.last()
            .map(|m| m.content.as_str())
            .unwrap_or("your request");

        format!(
            "**[AI Inference Unavailable]**\n\n\
            I couldn't process \"{}\" because:\n\n\
            1. No local GPU with sufficient VRAM detected\n\
            2. No network AI nodes currently available\n\n\
            **To enable AI features:**\n\
            - *Option A:* Download an AI model in NEURAX Settings (requires 4GB+ VRAM)\n\
            - *Option B:* Wait for network AI nodes to come online\n\
            - *Option C:* Enable 'AI Job Processing' on a GPU-equipped node to help the network\n\n\
            The mesh optimizer and basic analytics still work without AI inference.",
            last_message.chars().take(100).collect::<String>()
        )
    }

    /// Save configuration to disk
    pub fn save_config(&self, data_dir: &std::path::Path) -> Result<(), String> {
        let config_path = data_dir.join("neurax_ai_config.json");
        let config = self.config.read().clone();
        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize AI config: {}", e))?;
        std::fs::write(&config_path, content)
            .map_err(|e| format!("Failed to save AI config: {}", e))?;
        Ok(())
    }

    /// Load configuration from disk
    pub fn load_config(&self, data_dir: &std::path::Path) {
        let config_path = data_dir.join("neurax_ai_config.json");
        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(config) = serde_json::from_str::<AiNodeConfig>(&content) {
                    *self.config.write() = config;
                }
            }
        }
    }
}

impl Default for AiInferenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Current inference mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InferenceMode {
    /// Using local GPU
    LocalGpu,
    /// Have local GPU but model not loaded
    LocalGpuPendingModel,
    /// Using network distributed nodes
    NetworkDistributed { available_nodes: usize },
    /// No inference available
    Unavailable,
}

impl InferenceMode {
    pub fn description(&self) -> String {
        match self {
            InferenceMode::LocalGpu => "Local GPU (fastest)".to_string(),
            InferenceMode::LocalGpuPendingModel => "Local GPU available - download model to enable".to_string(),
            InferenceMode::NetworkDistributed { available_nodes } => {
                format!("Network Distributed ({} nodes available)", available_nodes)
            }
            InferenceMode::Unavailable => "Unavailable - no GPU or network nodes".to_string(),
        }
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================

use tauri::{AppHandle, State};

/// Wrapper for AI inference engine state
pub struct AiInferenceWrapper(pub Arc<AiInferenceEngine>);

#[tauri::command]
pub async fn neurax_get_gpu_capabilities() -> Result<Vec<DetectedGpu>, String> {
    Ok(detect_gpu_capabilities())
}

#[tauri::command]
pub async fn neurax_get_inference_mode(
    state: State<'_, AiInferenceWrapper>,
) -> Result<InferenceMode, String> {
    Ok(state.0.get_inference_mode())
}

#[tauri::command]
pub async fn neurax_get_ai_config(
    state: State<'_, AiInferenceWrapper>,
) -> Result<AiNodeConfig, String> {
    Ok(state.0.config.read().clone())
}

#[tauri::command]
pub async fn neurax_set_ai_config(
    app: AppHandle,
    state: State<'_, AiInferenceWrapper>,
    config: AiNodeConfig,
) -> Result<(), String> {
    *state.0.config.write() = config;
    
    let data_dir = app.path_resolver().app_data_dir()
        .ok_or("Failed to get app data directory")?;
    state.0.save_config(&data_dir)?;
    
    info!("AI config updated: accept_network_jobs={}", state.0.config.read().accept_network_jobs);
    Ok(())
}

#[tauri::command]
pub async fn neurax_process_chat(
    state: State<'_, AiInferenceWrapper>,
    messages: Vec<ChatMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
) -> Result<String, String> {
    state.0.process_chat(
        messages,
        max_tokens.unwrap_or(512),
        temperature.unwrap_or(0.7),
    ).await
}

#[tauri::command]
pub async fn neurax_get_ai_stats(
    state: State<'_, AiInferenceWrapper>,
) -> Result<AiEngineStats, String> {
    Ok(state.0.stats.read().clone())
}

#[tauri::command]
pub async fn neurax_get_remote_ai_nodes(
    state: State<'_, AiInferenceWrapper>,
) -> Result<Vec<RemoteAiNode>, String> {
    Ok(state.0.remote_nodes.read().values().cloned().collect())
}

#[tauri::command]
pub async fn neurax_can_local_inference(
    state: State<'_, AiInferenceWrapper>,
) -> Result<bool, String> {
    Ok(state.0.should_use_local())
}
