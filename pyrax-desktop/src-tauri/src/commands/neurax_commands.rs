use tauri::{AppHandle, Manager, State};
use std::sync::Arc;
use parking_lot::RwLock;
use crate::commands::neurax::*;

/// Global NEURAX state
pub struct NeuraxStateWrapper(pub Arc<NeuraxState>);

// ============================================================================
// Tauri Commands
// ============================================================================

#[tauri::command]
pub async fn neurax_get_config(
    state: State<'_, NeuraxStateWrapper>,
) -> Result<NeuraxConfig, String> {
    Ok(state.0.config.read().clone())
}

#[tauri::command]
pub async fn neurax_set_config(
    app: AppHandle,
    state: State<'_, NeuraxStateWrapper>,
    config: NeuraxConfig,
) -> Result<(), String> {
    *state.0.config.write() = config;
    
    // Save to disk
    let data_dir = app.path_resolver().app_data_dir()
        .ok_or("Failed to get app data directory")?;
    state.0.save_config(&data_dir)?;
    
    Ok(())
}

#[tauri::command]
pub async fn neurax_set_enabled(
    app: AppHandle,
    state: State<'_, NeuraxStateWrapper>,
    enabled: bool,
) -> Result<(), String> {
    state.0.config.write().enabled = enabled;
    
    let data_dir = app.path_resolver().app_data_dir()
        .ok_or("Failed to get app data directory")?;
    state.0.save_config(&data_dir)?;
    
    Ok(())
}

/// Result of permission change with elevation status
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionChangeResult {
    pub success: bool,
    pub elevated: bool,
    pub message: String,
    pub permissions_changed: Vec<String>,
}

#[tauri::command]
pub async fn neurax_set_permissions(
    app: AppHandle,
    state: State<'_, NeuraxStateWrapper>,
    permissions: NeuraxPermissions,
    request_elevation: bool,
) -> Result<PermissionChangeResult, String> {
    use crate::commands::neurax_admin;
    
    let current_permissions = state.0.config.read().permissions.clone();
    let mut permissions_changed = Vec::new();
    let mut needs_elevation = false;
    
    // Check which sensitive permissions are being enabled
    if permissions.process_management && !current_permissions.process_management {
        permissions_changed.push("process_management".to_string());
        needs_elevation = true;
    }
    if permissions.memory_optimization && !current_permissions.memory_optimization {
        permissions_changed.push("memory_optimization".to_string());
        needs_elevation = true;
    }
    if permissions.auto_apply && !current_permissions.auto_apply {
        permissions_changed.push("auto_apply".to_string());
        needs_elevation = true;
    }
    if permissions.network_tuning && !current_permissions.network_tuning {
        permissions_changed.push("network_tuning".to_string());
    }
    if permissions.power_settings && !current_permissions.power_settings {
        permissions_changed.push("power_settings".to_string());
    }
    
    // If enabling sensitive permissions and elevation is requested
    if needs_elevation && request_elevation && !permissions_changed.is_empty() {
        let permission_list = permissions_changed.join(", ");
        let elevation_result = neurax_admin::request_elevation(
            &permission_list,
            "These permissions allow NEURAX to make system-level changes to optimize your node's performance.",
        ).await;
        
        if !elevation_result.elevated {
            return Ok(PermissionChangeResult {
                success: false,
                elevated: false,
                message: elevation_result.message,
                permissions_changed: vec![],
            });
        }
    }
    
    // Apply permissions
    state.0.config.write().permissions = permissions;
    
    let data_dir = app.path_resolver().app_data_dir()
        .ok_or("Failed to get app data directory")?;
    state.0.save_config(&data_dir)?;
    
    Ok(PermissionChangeResult {
        success: true,
        elevated: needs_elevation,
        message: if permissions_changed.is_empty() {
            "Permissions updated".to_string()
        } else {
            format!("Permissions granted: {}", permissions_changed.join(", "))
        },
        permissions_changed,
    })
}

#[tauri::command]
pub async fn neurax_get_system_metrics(
    state: State<'_, NeuraxStateWrapper>,
) -> Result<SystemMetrics, String> {
    let metrics = collect_system_metrics(&state.0);
    
    // Store in history
    let mut history = state.0.metrics_history.write();
    history.push(metrics.clone());
    if history.len() > 60 {
        history.remove(0);
    }
    
    Ok(metrics)
}

#[tauri::command]
pub async fn neurax_get_metrics_history(
    state: State<'_, NeuraxStateWrapper>,
) -> Result<Vec<SystemMetrics>, String> {
    Ok(state.0.metrics_history.read().clone())
}

#[tauri::command]
pub async fn neurax_analyze_logs(
    logs: Vec<String>,
) -> Result<LogAnalysis, String> {
    Ok(analyze_logs(&logs))
}

#[tauri::command]
pub async fn neurax_generate_insights(
    state: State<'_, NeuraxStateWrapper>,
    logs: Vec<String>,
    node_status: Option<serde_json::Value>,
) -> Result<Vec<NeuraxInsight>, String> {
    let metrics = collect_system_metrics(&state.0);
    let log_analysis = analyze_logs(&logs);
    let insights = generate_insights(&metrics, &log_analysis, node_status.as_ref());
    
    // Store insights
    *state.0.insights.write() = insights.clone();
    
    Ok(insights)
}

#[tauri::command]
pub async fn neurax_get_insights(
    state: State<'_, NeuraxStateWrapper>,
) -> Result<Vec<NeuraxInsight>, String> {
    Ok(state.0.insights.read().clone())
}

#[tauri::command]
pub async fn neurax_dismiss_insight(
    state: State<'_, NeuraxStateWrapper>,
    insight_id: String,
) -> Result<(), String> {
    let mut insights = state.0.insights.write();
    if let Some(insight) = insights.iter_mut().find(|i| i.id == insight_id) {
        insight.dismissed = true;
    }
    Ok(())
}

#[tauri::command]
pub async fn neurax_chat(
    state: State<'_, NeuraxStateWrapper>,
    message: String,
    logs: Vec<String>,
    node_status: Option<serde_json::Value>,
) -> Result<ChatMessage, String> {
    let metrics = collect_system_metrics(&state.0);
    let log_analysis = analyze_logs(&logs);
    
    // Generate response
    let response_content = generate_chat_response(&message, &metrics, &log_analysis, node_status.as_ref());
    
    let now = chrono::Utc::now().timestamp();
    
    // Store user message
    let user_msg = ChatMessage {
        id: format!("user-{}", now),
        timestamp: now,
        role: "user".to_string(),
        content: message,
    };
    
    // Store assistant response
    let assistant_msg = ChatMessage {
        id: format!("assistant-{}", now),
        timestamp: now,
        role: "assistant".to_string(),
        content: response_content,
    };
    
    {
        let mut history = state.0.chat_history.write();
        history.push(user_msg);
        history.push(assistant_msg.clone());
        
        // Keep last 50 messages
        if history.len() > 50 {
            let start = history.len() - 50;
            *history = history.drain(start..).collect();
        }
    }
    
    Ok(assistant_msg)
}

#[tauri::command]
pub async fn neurax_get_chat_history(
    state: State<'_, NeuraxStateWrapper>,
) -> Result<Vec<ChatMessage>, String> {
    Ok(state.0.chat_history.read().clone())
}

#[tauri::command]
pub async fn neurax_clear_chat(
    state: State<'_, NeuraxStateWrapper>,
) -> Result<(), String> {
    state.0.chat_history.write().clear();
    Ok(())
}

#[tauri::command]
pub async fn neurax_execute_action(
    state: State<'_, NeuraxStateWrapper>,
    action_id: String,
) -> Result<ActionResult, String> {
    let permissions = state.0.config.read().permissions.clone();
    Ok(execute_action(&action_id, &permissions).await)
}

#[tauri::command]
pub async fn neurax_get_quick_insights(
    state: State<'_, NeuraxStateWrapper>,
    logs: Vec<String>,
    node_status: Option<serde_json::Value>,
) -> Result<QuickInsights, String> {
    let metrics = collect_system_metrics(&state.0);
    let log_analysis = analyze_logs(&logs);
    
    // Calculate scores
    let system_score = calculate_system_score(&metrics);
    let network_score = calculate_network_score(&log_analysis, node_status.as_ref());
    let mining_score = calculate_mining_score(&metrics, &log_analysis);
    
    // Get top insights
    let insights = generate_insights(&metrics, &log_analysis, node_status.as_ref());
    let top_insights: Vec<NeuraxInsight> = insights.into_iter()
        .filter(|i| !i.dismissed && i.severity != InsightSeverity::Info)
        .take(3)
        .collect();
    
    Ok(QuickInsights {
        system_score,
        network_score,
        mining_score,
        top_insights,
        error_count: log_analysis.error_count,
        warning_count: log_analysis.warning_count,
    })
}

/// Quick insights for dashboard panel
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuickInsights {
    pub system_score: u32,
    pub network_score: u32,
    pub mining_score: u32,
    pub top_insights: Vec<NeuraxInsight>,
    pub error_count: usize,
    pub warning_count: usize,
}

fn calculate_system_score(metrics: &SystemMetrics) -> u32 {
    let mut score = 100u32;
    
    // CPU penalty
    if metrics.cpu_usage > 90.0 {
        score = score.saturating_sub(30);
    } else if metrics.cpu_usage > 80.0 {
        score = score.saturating_sub(15);
    }
    
    // Memory penalty
    if metrics.memory_percent > 90.0 {
        score = score.saturating_sub(30);
    } else if metrics.memory_percent > 80.0 {
        score = score.saturating_sub(15);
    }
    
    // CPU temp penalty
    if let Some(temp) = metrics.cpu_temp {
        if temp > 90.0 {
            score = score.saturating_sub(20);
        } else if temp > 80.0 {
            score = score.saturating_sub(10);
        }
    }
    
    score
}

fn calculate_network_score(log_analysis: &LogAnalysis, node_status: Option<&serde_json::Value>) -> u32 {
    let mut score = 100u32;
    
    // Network errors penalty
    let network_errors: usize = log_analysis.patterns.iter()
        .filter(|p| p.severity == "network")
        .map(|p| p.count)
        .sum();
    
    if network_errors > 20 {
        score = score.saturating_sub(40);
    } else if network_errors > 10 {
        score = score.saturating_sub(20);
    } else if network_errors > 5 {
        score = score.saturating_sub(10);
    }
    
    // Peer count check
    if let Some(status) = node_status {
        if let Some(peers) = status.get("peerCount").and_then(|v| v.as_i64()) {
            if peers == 0 {
                score = score.saturating_sub(50);
            } else if peers < 3 {
                score = score.saturating_sub(20);
            }
        }
    }
    
    score
}

fn calculate_mining_score(metrics: &SystemMetrics, log_analysis: &LogAnalysis) -> u32 {
    let mut score = 100u32;
    
    // GPU temp penalty
    if let Some(temp) = metrics.gpu_temp {
        if temp > 90.0 {
            score = score.saturating_sub(40);
        } else if temp > 85.0 {
            score = score.saturating_sub(25);
        } else if temp > 80.0 {
            score = score.saturating_sub(10);
        }
    }
    
    // Mining errors penalty
    let mining_errors: usize = log_analysis.patterns.iter()
        .filter(|p| p.severity == "mining")
        .map(|p| p.count)
        .sum();
    
    if mining_errors > 10 {
        score = score.saturating_sub(30);
    } else if mining_errors > 5 {
        score = score.saturating_sub(15);
    }
    
    score
}
