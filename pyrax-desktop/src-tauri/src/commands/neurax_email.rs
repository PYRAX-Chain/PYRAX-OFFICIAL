// NEURAX Error Logging with Brevo Email Integration
// Sends error logs to neurax-errors@pyrax.org every 5 minutes

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use parking_lot::RwLock;
use chrono::{DateTime, Utc};
use tokio::time::{interval, Duration};

/// Brevo API configuration
/// API Key should be set via environment variable: BREVO_API_KEY
/// Or configured in the app settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrevoConfig {
    pub api_key: Option<String>,
    pub enabled: bool,
    pub send_interval_secs: u64,
    pub recipient_email: String,
    pub sender_email: String,
    pub sender_name: String,
}

impl Default for BrevoConfig {
    fn default() -> Self {
        Self {
            api_key: std::env::var("BREVO_API_KEY").ok(),
            enabled: false,
            send_interval_secs: 300, // 5 minutes
            recipient_email: "neurax-errors@pyrax.org".to_string(),
            sender_email: "neurax@pyrax.org".to_string(),
            sender_name: "NEURAX AI System".to_string(),
        }
    }
}

/// Error log entry for NEURAX
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuraxErrorLog {
    pub timestamp: i64,
    pub level: String,
    pub category: String,
    pub message: String,
    pub context: Option<String>,
}

/// Error log buffer that accumulates errors for batch sending
pub struct NeuraxErrorBuffer {
    pub config: RwLock<BrevoConfig>,
    pub errors: RwLock<Vec<NeuraxErrorLog>>,
    pub last_sent: RwLock<Option<i64>>,
}

impl NeuraxErrorBuffer {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(BrevoConfig::default()),
            errors: RwLock::new(Vec::new()),
            last_sent: RwLock::new(None),
        }
    }

    /// Add an error to the buffer
    pub fn add_error(&self, level: &str, category: &str, message: &str, context: Option<&str>) {
        let entry = NeuraxErrorLog {
            timestamp: Utc::now().timestamp(),
            level: level.to_string(),
            category: category.to_string(),
            message: message.to_string(),
            context: context.map(|s| s.to_string()),
        };
        
        let mut errors = self.errors.write();
        errors.push(entry);
        
        // Keep only last 1000 errors to prevent memory bloat
        let len = errors.len();
        if len > 1000 {
            errors.drain(0..len - 1000);
        }
    }

    /// Get all buffered errors and clear the buffer
    pub fn drain_errors(&self) -> Vec<NeuraxErrorLog> {
        let mut errors = self.errors.write();
        std::mem::take(&mut *errors)
    }

    /// Get error count
    pub fn error_count(&self) -> usize {
        self.errors.read().len()
    }

    /// Update Brevo configuration
    pub fn set_config(&self, config: BrevoConfig) {
        *self.config.write() = config;
    }

    /// Get current configuration
    pub fn get_config(&self) -> BrevoConfig {
        self.config.read().clone()
    }
}

/// Format errors as HTML email body
fn format_errors_html(errors: &[NeuraxErrorLog], node_info: &str) -> String {
    let error_count = errors.iter().filter(|e| e.level == "error" || e.level == "ERROR").count();
    let warning_count = errors.iter().filter(|e| e.level == "warn" || e.level == "WARN").count();
    
    let mut html = format!(r#"
<!DOCTYPE html>
<html>
<head>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #1a1a1a; color: #e0e0e0; padding: 20px; }}
        .header {{ background: linear-gradient(135deg, #7c3aed, #2563eb); padding: 20px; border-radius: 12px; margin-bottom: 20px; }}
        .header h1 {{ margin: 0; color: white; }}
        .stats {{ display: flex; gap: 20px; margin-bottom: 20px; }}
        .stat {{ background: #2a2a2a; padding: 15px; border-radius: 8px; flex: 1; }}
        .stat-value {{ font-size: 24px; font-weight: bold; }}
        .error {{ color: #ef4444; }}
        .warning {{ color: #f59e0b; }}
        .info {{ color: #3b82f6; }}
        .log-entry {{ background: #2a2a2a; padding: 12px; border-radius: 8px; margin-bottom: 8px; border-left: 4px solid #555; }}
        .log-entry.error {{ border-left-color: #ef4444; }}
        .log-entry.warn {{ border-left-color: #f59e0b; }}
        .timestamp {{ color: #888; font-size: 12px; }}
        .category {{ background: #3a3a3a; padding: 2px 8px; border-radius: 4px; font-size: 12px; margin-left: 8px; }}
        .message {{ margin-top: 8px; }}
        .node-info {{ background: #2a2a2a; padding: 15px; border-radius: 8px; margin-bottom: 20px; font-size: 14px; color: #888; }}
    </style>
</head>
<body>
    <div class="header">
        <h1>🧠 NEURAX Error Report</h1>
        <p style="margin: 5px 0 0 0; color: rgba(255,255,255,0.8);">Generated at {}</p>
    </div>
    
    <div class="node-info">
        <strong>Node Info:</strong> {}
    </div>
    
    <div class="stats">
        <div class="stat">
            <div class="stat-value error">{}</div>
            <div>Errors</div>
        </div>
        <div class="stat">
            <div class="stat-value warning">{}</div>
            <div>Warnings</div>
        </div>
        <div class="stat">
            <div class="stat-value">{}</div>
            <div>Total Entries</div>
        </div>
    </div>
    
    <h2>Log Entries</h2>
"#, 
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        node_info,
        error_count,
        warning_count,
        errors.len()
    );

    // Add each error entry
    for error in errors.iter().rev().take(100) {  // Show last 100 errors
        let level_class = match error.level.to_lowercase().as_str() {
            "error" => "error",
            "warn" | "warning" => "warn",
            _ => "",
        };
        
        let timestamp = DateTime::from_timestamp(error.timestamp, 0)
            .map(|dt| dt.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        
        html.push_str(&format!(r#"
    <div class="log-entry {}">
        <span class="timestamp">{}</span>
        <span class="category">{}</span>
        <span class="level {}">[{}]</span>
        <div class="message">{}</div>
        {}
    </div>
"#,
            level_class,
            timestamp,
            error.category,
            level_class,
            error.level.to_uppercase(),
            html_escape(&error.message),
            error.context.as_ref().map(|c| format!("<div style='color:#666;font-size:12px;margin-top:4px;'>{}</div>", html_escape(c))).unwrap_or_default()
        ));
    }

    html.push_str(r#"
</body>
</html>
"#);
    
    html
}

/// Escape HTML special characters
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Send error report via Brevo API
pub async fn send_error_report(
    config: &BrevoConfig,
    errors: &[NeuraxErrorLog],
    node_info: &str,
) -> Result<(), String> {
    let api_key = config.api_key.as_ref()
        .ok_or("Brevo API key not configured")?;
    
    if errors.is_empty() {
        return Ok(()); // Nothing to send
    }
    
    let html_content = format_errors_html(errors, node_info);
    let error_count = errors.iter().filter(|e| e.level.to_lowercase() == "error").count();
    let subject = format!(
        "NEURAX Error Report - {} errors, {} total entries",
        error_count,
        errors.len()
    );
    
    // Brevo API payload
    let payload = serde_json::json!({
        "sender": {
            "name": config.sender_name,
            "email": config.sender_email
        },
        "to": [{
            "email": config.recipient_email,
            "name": "NEURAX Errors"
        }],
        "subject": subject,
        "htmlContent": html_content
    });
    
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.brevo.com/v3/smtp/email")
        .header("accept", "application/json")
        .header("api-key", api_key)
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send email: {}", e))?;
    
    if response.status().is_success() {
        tracing::info!("NEURAX error report sent successfully ({} entries)", errors.len());
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!("Brevo API error {}: {}", status, body))
    }
}

/// Start the background error reporting task
pub fn start_error_reporter(
    buffer: Arc<NeuraxErrorBuffer>,
    node_info_fn: impl Fn() -> String + Send + Sync + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60)); // Check every minute
        let mut last_send = std::time::Instant::now();
        
        loop {
            ticker.tick().await;
            
            let config = buffer.get_config();
            if !config.enabled || config.api_key.is_none() {
                continue;
            }
            
            // Check if it's time to send (every 5 minutes by default)
            let send_interval = Duration::from_secs(config.send_interval_secs);
            if last_send.elapsed() < send_interval {
                continue;
            }
            
            let error_count = buffer.error_count();
            if error_count == 0 {
                continue; // No errors to send
            }
            
            // Drain and send errors
            let errors = buffer.drain_errors();
            let node_info = node_info_fn();
            
            match send_error_report(&config, &errors, &node_info).await {
                Ok(_) => {
                    *buffer.last_sent.write() = Some(Utc::now().timestamp());
                    last_send = std::time::Instant::now();
                }
                Err(e) => {
                    tracing::error!("Failed to send NEURAX error report: {}", e);
                    // Put errors back in buffer for retry
                    let mut buf = buffer.errors.write();
                    for error in errors {
                        buf.push(error);
                    }
                }
            }
        }
    })
}

// ============================================================================
// Tauri Commands for Brevo Configuration
// ============================================================================

use tauri::{AppHandle, State};

/// Wrapper for error buffer state
pub struct NeuraxErrorBufferWrapper(pub Arc<NeuraxErrorBuffer>);

#[tauri::command]
pub async fn neurax_get_brevo_config(
    state: State<'_, NeuraxErrorBufferWrapper>,
) -> Result<BrevoConfig, String> {
    let mut config = state.0.get_config();
    // Don't expose the API key to frontend
    config.api_key = config.api_key.map(|_| "***configured***".to_string());
    Ok(config)
}

#[tauri::command]
pub async fn neurax_set_brevo_config(
    app: AppHandle,
    state: State<'_, NeuraxErrorBufferWrapper>,
    api_key: Option<String>,
    enabled: bool,
) -> Result<(), String> {
    let mut config = state.0.get_config();
    
    // Only update API key if a new one is provided
    if let Some(key) = api_key {
        if !key.is_empty() && key != "***configured***" {
            config.api_key = Some(key);
        }
    }
    
    config.enabled = enabled;
    state.0.set_config(config.clone());
    
    // Save to config file
    let data_dir = app.path_resolver().app_data_dir()
        .ok_or("Failed to get app data directory")?;
    let config_path = data_dir.join("neurax_brevo_config.json");
    
    // Don't save API key to disk in plain text - use environment variable
    let save_config = BrevoConfig {
        api_key: None, // API key should be set via BREVO_API_KEY env var
        ..config
    };
    
    let content = serde_json::to_string_pretty(&save_config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&config_path, content)
        .map_err(|e| format!("Failed to save config: {}", e))?;
    
    Ok(())
}

#[tauri::command]
pub async fn neurax_get_error_stats(
    state: State<'_, NeuraxErrorBufferWrapper>,
) -> Result<serde_json::Value, String> {
    let errors = state.0.errors.read();
    let last_sent = *state.0.last_sent.read();
    
    let error_count = errors.iter().filter(|e| e.level.to_lowercase() == "error").count();
    let warning_count = errors.iter().filter(|e| e.level.to_lowercase() == "warn" || e.level.to_lowercase() == "warning").count();
    
    Ok(serde_json::json!({
        "totalBuffered": errors.len(),
        "errorCount": error_count,
        "warningCount": warning_count,
        "lastSent": last_sent,
        "enabled": state.0.get_config().enabled
    }))
}

#[tauri::command]
pub async fn neurax_test_email(
    state: State<'_, NeuraxErrorBufferWrapper>,
) -> Result<String, String> {
    let config = state.0.get_config();
    
    if config.api_key.is_none() {
        return Err("Brevo API key not configured. Set BREVO_API_KEY environment variable.".to_string());
    }
    
    let test_errors = vec![
        NeuraxErrorLog {
            timestamp: Utc::now().timestamp(),
            level: "INFO".to_string(),
            category: "test".to_string(),
            message: "This is a test message from NEURAX".to_string(),
            context: Some("Test email to verify Brevo configuration".to_string()),
        }
    ];
    
    send_error_report(&config, &test_errors, "Test Node - NEURAX Email Test").await?;
    
    Ok("Test email sent successfully!".to_string())
}
