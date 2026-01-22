#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod node;
mod wallet;
mod miner;
mod state;
mod rpc;

use state::AppState;
use std::sync::Arc;
use parking_lot::Mutex;
use tauri::Manager;
use tracing::{info, warn, error};
use tracing_subscriber::{self, fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use commands::neurax::NeuraxState;
use commands::neurax_commands::NeuraxStateWrapper;
use commands::neurax_email::{NeuraxErrorBuffer, NeuraxErrorBufferWrapper};
use commands::neurax_llm::{LlmState, LlmStateWrapper};
use commands::neurax_mesh_optimizer::{MeshOptimizer, MeshOptimizerWrapper};
use commands::neurax_inference::{AiInferenceEngine, AiInferenceWrapper};

/// Check if Visual C++ Runtime is installed (Windows only)
#[cfg(target_os = "windows")]
fn check_and_install_vcruntime() {
    use std::process::Command;
    
    // Check registry for VC++ 2015-2022 Redistributable
    let check_x64 = Command::new("reg")
        .args(["query", r"HKLM\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64", "/v", "Installed"])
        .output();
    
    let x64_installed = match check_x64 {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("0x1")
        }
        Err(_) => false,
    };
    
    if x64_installed {
        info!("Visual C++ Runtime (x64) is installed");
        return;
    }
    
    warn!("Visual C++ Runtime not found, attempting to install...");
    
    // Try to find bundled VC++ redistributable in resources
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));
    
    if let Some(dir) = exe_dir {
        let vcredist_path = dir.join("resources").join("vc_redist.x64.exe");
        if vcredist_path.exists() {
            info!("Installing Visual C++ Runtime from bundled installer...");
            let result = Command::new(&vcredist_path)
                .args(["/install", "/passive", "/norestart"])
                .status();
            
            match result {
                Ok(status) => {
                    if status.success() || status.code() == Some(1638) || status.code() == Some(3010) {
                        info!("Visual C++ Runtime installed successfully");
                    } else {
                        warn!("Visual C++ Runtime installation returned code: {:?}", status.code());
                    }
                }
                Err(e) => warn!("Failed to run VC++ installer: {}", e),
            }
            return;
        }
    }
    
    // If bundled installer not found, show a message dialog
    warn!("VC++ redistributable not bundled. User may need to install manually.");
    
    // Try to open Microsoft download page
    if let Err(e) = open::that("https://aka.ms/vs/17/release/vc_redist.x64.exe") {
        warn!("Failed to open VC++ download page: {}", e);
    }
}

#[cfg(not(target_os = "windows"))]
fn check_and_install_vcruntime() {
    // No-op on non-Windows platforms
}

/// Show a native error dialog on Windows
#[cfg(target_os = "windows")]
fn show_error_dialog(title: &str, message: &str) {
    use std::ptr::null_mut;
    let msg_wide: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, utype: u32) -> i32;
        }
        MessageBoxW(null_mut(), msg_wide.as_ptr(), title_wide.as_ptr(), 0x10); // MB_ICONERROR
    }
}

#[cfg(not(target_os = "windows"))]
fn show_error_dialog(_title: &str, message: &str) {
    eprintln!("ERROR: {}", message);
}

/// Run startup diagnostics and return any issues found
fn run_startup_diagnostics() -> Vec<String> {
    let mut issues = Vec::new();
    
    // Check data directory
    let data_dir = directories::ProjectDirs::from("org", "pyrax", "PYRAX Desktop")
        .map(|d| d.data_dir().to_path_buf());
    
    match &data_dir {
        Some(dir) => {
            if std::fs::create_dir_all(dir).is_err() {
                issues.push(format!("Cannot create data directory: {:?}", dir));
            }
        }
        None => {
            issues.push("Cannot determine data directory".to_string());
        }
    }
    
    // Check available disk space (warn if < 100MB)
    #[cfg(target_os = "windows")]
    {
        if let Some(dir) = &data_dir {
            if let Some(root) = dir.ancestors().last() {
                // On Windows, check disk space using sysinfo would be ideal
                // For now, just log the path
                info!("Data directory root: {:?}", root);
            }
        }
    }
    
    issues
}

fn main() {
    // Wrap everything in a catch_unwind to prevent silent crashes
    let result = std::panic::catch_unwind(|| {
        run_app();
    });
    
    if let Err(e) = result {
        let panic_msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };
        
        let msg = format!("Inferno Node crashed unexpectedly:\n\n{}\n\nPlease check the log file for details.", panic_msg);
        show_error_dialog("Inferno Node Crash", &msg);
        std::process::exit(1);
    }
}

fn run_app() {
    // WINDOWS FIX: Initialize logging safely - write to file on Windows to avoid console issues
    #[cfg(target_os = "windows")]
    {
        // On Windows GUI apps, there's no console, so we write logs to a file
        let log_dir = directories::ProjectDirs::from("org", "pyrax", "PYRAX Desktop")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let _ = std::fs::create_dir_all(&log_dir);
        let log_file = log_dir.join("inferno-desktop.log");
        
        // Try to set up file logging, fall back to no logging if it fails
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
        {
            let file_layer = fmt::layer()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false);
            let _ = tracing_subscriber::registry()
                .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
                .with(file_layer)
                .try_init();
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    {
        // On Unix, console logging works fine
        let _ = tracing_subscriber::fmt::try_init();
    }
    
    info!("Starting PYRAX Desktop v{}", env!("CARGO_PKG_VERSION"));
    
    // Run startup diagnostics
    let issues = run_startup_diagnostics();
    for issue in &issues {
        warn!("Startup issue: {}", issue);
    }
    
    // Check and install VC++ runtime if needed (Windows only)
    check_and_install_vcruntime();
    
    info!("Startup diagnostics complete, initializing app...");

    // Initialize application state
    let app_state = Arc::new(Mutex::new(AppState::new()));
    
    // Initialize NEURAX AI state
    let neurax_state = Arc::new(NeuraxState::new());
    
    // Initialize NEURAX error buffer for Brevo email reporting
    let neurax_error_buffer = Arc::new(NeuraxErrorBuffer::new());
    
    // Initialize NEURAX LLM state for local AI inference
    let neurax_llm_state = Arc::new(LlmState::new());
    
    // Initialize NEURAX Mesh Network Optimizer
    let neurax_mesh_optimizer = Arc::new(MeshOptimizer::new());
    
    // Initialize NEURAX Distributed AI Inference Engine
    let neurax_ai_engine = Arc::new(AiInferenceEngine::new());

    tauri::Builder::default()
        .manage(app_state)
        .manage(NeuraxStateWrapper(neurax_state.clone()))
        .manage(NeuraxErrorBufferWrapper(neurax_error_buffer.clone()))
        .manage(LlmStateWrapper(neurax_llm_state.clone()))
        .manage(MeshOptimizerWrapper(neurax_mesh_optimizer.clone()))
        .manage(AiInferenceWrapper(neurax_ai_engine.clone()))
        .invoke_handler(tauri::generate_handler![
            // Node commands
            commands::node::start_node,
            commands::node::stop_node,
            commands::node::get_node_status,
            commands::node::get_chain_info,
            commands::node::get_peers,
            commands::node::get_network_mesh,
            commands::node::measure_bootnode_latency,
            
            // Wallet commands
            commands::wallet::create_wallet,
            commands::wallet::unlock_wallet,
            commands::wallet::lock_wallet,
            commands::wallet::get_addresses,
            commands::wallet::create_address,
            commands::wallet::get_balance,
            commands::wallet::send_transaction,
            commands::wallet::get_transactions,
            commands::wallet::import_mnemonic,
            commands::wallet::export_mnemonic,
            
            // Miner commands
            commands::miner::start_miner,
            commands::miner::stop_miner,
            commands::miner::get_miner_status,
            commands::miner::get_hashrate,
            commands::miner::detect_gpus,
            commands::miner::benchmark_gpu,
            
            // Explorer commands
            commands::explorer::get_block,
            commands::explorer::get_transaction,
            commands::explorer::get_recent_blocks,
            commands::explorer::get_bootnode_info,
            commands::explorer::get_blocks_paginated,
            commands::explorer::get_network_stats,
            commands::explorer::get_address_info,
            commands::explorer::search_explorer,
            
            // Settings commands
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::get_data_dir,
            commands::settings::set_data_dir,
            commands::settings::browse_directory,
            commands::settings::check_firewall_status,
            commands::settings::configure_firewall,
            commands::settings::remove_firewall_rules,
            commands::settings::wipe_chain_data,
            commands::settings::get_chain_data_size,
            commands::settings::test_port_connectivity,
            commands::settings::get_network_diagnostics,
            
            // Remote logging commands
            commands::node::get_remote_server_logs,
            commands::node::start_remote_log_stream,
            
            // Connection watchdog commands (self-healing)
            commands::node::start_connection_watchdog,
            commands::node::stop_connection_watchdog,
            
            // Data management commands
            commands::node::clear_local_data,
            commands::node::get_local_data_size,
            
            // Updater commands
            commands::updater::check_for_updates,
            commands::updater::install_update,
            commands::updater::get_app_version,
            commands::updater::check_version_compatibility,
            
            // NEURAX AI commands
            commands::neurax_commands::neurax_get_config,
            commands::neurax_commands::neurax_set_config,
            commands::neurax_commands::neurax_set_enabled,
            commands::neurax_commands::neurax_set_permissions,
            commands::neurax_commands::neurax_get_system_metrics,
            commands::neurax_commands::neurax_get_metrics_history,
            commands::neurax_commands::neurax_analyze_logs,
            commands::neurax_commands::neurax_generate_insights,
            commands::neurax_commands::neurax_get_insights,
            commands::neurax_commands::neurax_dismiss_insight,
            commands::neurax_commands::neurax_chat,
            commands::neurax_commands::neurax_get_chat_history,
            commands::neurax_commands::neurax_clear_chat,
            commands::neurax_commands::neurax_execute_action,
            commands::neurax_commands::neurax_get_quick_insights,
            
            // NEURAX Brevo email commands
            commands::neurax_email::neurax_get_brevo_config,
            commands::neurax_email::neurax_set_brevo_config,
            commands::neurax_email::neurax_get_error_stats,
            commands::neurax_email::neurax_test_email,
            
            // NEURAX LLM commands
            commands::neurax_llm::neurax_get_llm_config,
            commands::neurax_llm::neurax_set_llm_config,
            commands::neurax_llm::neurax_detect_gpus,
            commands::neurax_llm::neurax_get_available_models,
            commands::neurax_llm::neurax_download_model,
            commands::neurax_llm::neurax_get_download_status,
            commands::neurax_llm::neurax_is_model_loaded,
            
            // NEURAX Admin privilege commands
            commands::neurax_admin::neurax_check_admin,
            commands::neurax_admin::neurax_request_elevation,
            commands::neurax_admin::neurax_get_permission_info,
            
            // NEURAX Mesh Optimizer commands
            commands::neurax_mesh_optimizer::neurax_get_mesh_health,
            commands::neurax_mesh_optimizer::neurax_get_optimization_action,
            commands::neurax_mesh_optimizer::neurax_get_mesh_summary,
            commands::neurax_mesh_optimizer::neurax_set_optimizer_config,
            commands::neurax_mesh_optimizer::neurax_toggle_optimizer,
            
            // NEURAX Distributed AI Inference commands
            commands::neurax_inference::neurax_get_gpu_capabilities,
            commands::neurax_inference::neurax_get_inference_mode,
            commands::neurax_inference::neurax_get_ai_config,
            commands::neurax_inference::neurax_set_ai_config,
            commands::neurax_inference::neurax_process_chat,
            commands::neurax_inference::neurax_get_ai_stats,
            commands::neurax_inference::neurax_get_remote_ai_nodes,
            commands::neurax_inference::neurax_can_local_inference,
        ])
        .setup(|app| {
            info!("Application setup complete");
            
            // Get data directory - use fallback if Tauri can't provide one
            let app_handle = app.handle();
            let data_dir = app_handle.path_resolver()
                .app_data_dir()
                .unwrap_or_else(|| {
                    warn!("Could not get app data directory from Tauri, using fallback");
                    directories::ProjectDirs::from("org", "pyrax", "PYRAX Desktop")
                        .map(|d| d.data_dir().to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("./pyrax-data"))
                });
            
            info!("Data directory: {:?}", data_dir);
            
            // Create data directory if it doesn't exist
            std::fs::create_dir_all(&data_dir).ok();
            
            // Load NEURAX config
            let neurax = app.state::<NeuraxStateWrapper>();
            neurax.0.load_config(&data_dir);
            info!("NEURAX AI system initialized");
            
            // Load Brevo config and start error reporter
            let error_buffer = app.state::<NeuraxErrorBufferWrapper>();
            let brevo_config_path = data_dir.join("neurax_brevo_config.json");
            if brevo_config_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&brevo_config_path) {
                    if let Ok(mut config) = serde_json::from_str::<commands::neurax_email::BrevoConfig>(&content) {
                        // API key should come from environment variable
                        config.api_key = std::env::var("BREVO_API_KEY").ok();
                        error_buffer.0.set_config(config);
                    }
                }
            }
            
            // Start background error reporter (sends every 5 minutes if enabled)
            let buffer_clone = error_buffer.0.clone();
            commands::neurax_email::start_error_reporter(
                buffer_clone,
                || format!("PYRAX Desktop v{}", env!("CARGO_PKG_VERSION"))
            );
            info!("NEURAX error reporter initialized");
            
            // Load LLM config
            let llm_state = app.state::<LlmStateWrapper>();
            llm_state.0.load_config(&data_dir);
            info!("NEURAX LLM system initialized");
            
            // Load AI Inference Engine config
            let ai_engine = app.state::<AiInferenceWrapper>();
            ai_engine.0.load_config(&data_dir);
            let mode = ai_engine.0.get_inference_mode();
            info!("NEURAX AI Inference Engine initialized: {:?}", mode);
            
            Ok(())
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            error!("Failed to run PYRAX Desktop: {}", e);
            // WINDOWS FIX: Show error dialog on Windows since there's no console
            #[cfg(target_os = "windows")]
            {
                use std::ptr::null_mut;
                let msg = format!("Failed to start Inferno Node:\n\n{}\n\nPlease ensure WebView2 is installed.", e);
                let msg_wide: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
                let title: Vec<u16> = "Inferno Node Error".encode_utf16().chain(std::iter::once(0)).collect();
                unsafe {
                    #[link(name = "user32")]
                    extern "system" {
                        fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, utype: u32) -> i32;
                    }
                    MessageBoxW(null_mut(), msg_wide.as_ptr(), title.as_ptr(), 0x10); // MB_ICONERROR
                }
            }
            std::process::exit(1);
        });
}
