// NEURAX Admin Privilege Elevation
// Handles OS-level privilege elevation when toggling NEURAX permissions
// This ensures users are prompted for admin/root access when enabling sensitive features

use serde::{Deserialize, Serialize};
use std::process::Command;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivilegeCheckResult {
    pub has_admin: bool,
    pub elevation_available: bool,
    pub os: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevationResult {
    pub success: bool,
    pub elevated: bool,
    pub message: String,
}

/// Check if the current process has admin/root privileges
pub fn check_admin_privileges() -> PrivilegeCheckResult {
    #[cfg(target_os = "windows")]
    {
        // On Windows, check if running as Administrator
        let is_admin = is_windows_admin();
        PrivilegeCheckResult {
            has_admin: is_admin,
            elevation_available: true,
            os: "windows".to_string(),
            message: if is_admin {
                "Running with Administrator privileges".to_string()
            } else {
                "Running as standard user. Admin elevation available via UAC.".to_string()
            },
        }
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, check if running as root or if user can sudo
        let is_root = unsafe { libc::geteuid() == 0 };
        PrivilegeCheckResult {
            has_admin: is_root,
            elevation_available: true,
            os: "macos".to_string(),
            message: if is_root {
                "Running with root privileges".to_string()
            } else {
                "Running as standard user. Admin elevation available via sudo/osascript.".to_string()
            },
        }
    }

    #[cfg(target_os = "linux")]
    {
        // On Linux, check if running as root or if pkexec is available
        let is_root = unsafe { libc::geteuid() == 0 };
        let pkexec_available = Command::new("which")
            .arg("pkexec")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        
        PrivilegeCheckResult {
            has_admin: is_root,
            elevation_available: pkexec_available,
            os: "linux".to_string(),
            message: if is_root {
                "Running with root privileges".to_string()
            } else if pkexec_available {
                "Running as standard user. Admin elevation available via pkexec.".to_string()
            } else {
                "Running as standard user. Install polkit for admin elevation.".to_string()
            },
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        PrivilegeCheckResult {
            has_admin: false,
            elevation_available: false,
            os: "unknown".to_string(),
            message: "Unsupported operating system for privilege elevation".to_string(),
        }
    }
}

#[cfg(target_os = "windows")]
fn is_windows_admin() -> bool {
    use std::mem;
    use std::ptr;
    
    // Use Windows API to check if running elevated
    // This is a simplified check - in production, use windows-sys crate
    let output = Command::new("net")
        .args(["session"])
        .output();
    
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Request elevation for a specific permission change
/// This will prompt the user with an OS-native dialog
pub async fn request_elevation(
    permission_name: &str,
    reason: &str,
) -> ElevationResult {
    #[cfg(target_os = "windows")]
    {
        request_elevation_windows(permission_name, reason).await
    }

    #[cfg(target_os = "macos")]
    {
        request_elevation_macos(permission_name, reason).await
    }

    #[cfg(target_os = "linux")]
    {
        request_elevation_linux(permission_name, reason).await
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        ElevationResult {
            success: false,
            elevated: false,
            message: "Privilege elevation not supported on this OS".to_string(),
        }
    }
}

#[cfg(target_os = "windows")]
async fn request_elevation_windows(permission_name: &str, reason: &str) -> ElevationResult {
    // On Windows, we use PowerShell to show a UAC-style confirmation dialog
    // The actual elevation would require restarting the app as admin
    
    let script = format!(
        r#"
        Add-Type -AssemblyName PresentationFramework
        $result = [System.Windows.MessageBox]::Show(
            'NEURAX requires administrator privileges to enable {}.`n`nReason: {}`n`nDo you want to grant these permissions?',
            'PYRAX NEURAX - Administrator Required',
            'YesNo',
            'Question'
        )
        if ($result -eq 'Yes') {{ exit 0 }} else {{ exit 1 }}
        "#,
        permission_name, reason
    );

    let output = Command::new("powershell")
        .args(["-ExecutionPolicy", "Bypass", "-Command", &script])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            // User approved - in a real implementation, we'd elevate here
            // For now, we just record the approval
            ElevationResult {
                success: true,
                elevated: true,
                message: format!("User approved elevation for: {}", permission_name),
            }
        }
        Ok(_) => ElevationResult {
            success: true,
            elevated: false,
            message: "User declined elevation request".to_string(),
        },
        Err(e) => ElevationResult {
            success: false,
            elevated: false,
            message: format!("Failed to show elevation dialog: {}", e),
        },
    }
}

#[cfg(target_os = "macos")]
async fn request_elevation_macos(permission_name: &str, reason: &str) -> ElevationResult {
    // On macOS, use osascript to show a native dialog with password prompt
    let script = format!(
        r#"display dialog "NEURAX requires administrator privileges to enable {}.

Reason: {}

Click OK to enter your password and grant these permissions." buttons {{"Cancel", "OK"}} default button "OK" with title "PYRAX NEURAX - Admin Required" with icon caution"#,
        permission_name, reason
    );

    let output = Command::new("osascript")
        .args(["-e", &script])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            // User clicked OK - now request actual elevation
            let auth_script = format!(
                r#"do shell script "echo 'NEURAX_ELEVATED'" with administrator privileges"#
            );
            
            let auth_output = Command::new("osascript")
                .args(["-e", &auth_script])
                .output();
            
            match auth_output {
                Ok(ao) if ao.status.success() => ElevationResult {
                    success: true,
                    elevated: true,
                    message: format!("Admin privileges granted for: {}", permission_name),
                },
                _ => ElevationResult {
                    success: true,
                    elevated: false,
                    message: "User cancelled password entry".to_string(),
                },
            }
        }
        Ok(_) => ElevationResult {
            success: true,
            elevated: false,
            message: "User declined elevation request".to_string(),
        },
        Err(e) => ElevationResult {
            success: false,
            elevated: false,
            message: format!("Failed to show elevation dialog: {}", e),
        },
    }
}

#[cfg(target_os = "linux")]
async fn request_elevation_linux(permission_name: &str, reason: &str) -> ElevationResult {
    // On Linux, use zenity or kdialog for GUI, or pkexec for elevation
    
    // First, show a confirmation dialog
    let zenity_available = Command::new("which")
        .arg("zenity")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    
    let user_approved = if zenity_available {
        let output = Command::new("zenity")
            .args([
                "--question",
                "--title=PYRAX NEURAX - Admin Required",
                &format!("--text=NEURAX requires administrator privileges to enable {}.\n\nReason: {}\n\nDo you want to grant these permissions?", permission_name, reason),
                "--width=400",
            ])
            .output();
        
        output.map(|o| o.status.success()).unwrap_or(false)
    } else {
        // Fallback: try kdialog
        let output = Command::new("kdialog")
            .args([
                "--title", "PYRAX NEURAX - Admin Required",
                "--yesno", &format!("NEURAX requires administrator privileges to enable {}.\n\nReason: {}\n\nDo you want to grant these permissions?", permission_name, reason),
            ])
            .output();
        
        output.map(|o| o.status.success()).unwrap_or(false)
    };

    if !user_approved {
        return ElevationResult {
            success: true,
            elevated: false,
            message: "User declined elevation request".to_string(),
        };
    }

    // User approved - try pkexec for actual elevation
    let pkexec_output = Command::new("pkexec")
        .args(["echo", "NEURAX_ELEVATED"])
        .output();

    match pkexec_output {
        Ok(o) if o.status.success() => ElevationResult {
            success: true,
            elevated: true,
            message: format!("Admin privileges granted for: {}", permission_name),
        },
        _ => ElevationResult {
            success: true,
            elevated: false,
            message: "User cancelled authentication or pkexec failed".to_string(),
        },
    }
}

/// Permission descriptions for elevation dialogs
pub fn get_permission_description(permission: &str) -> (&'static str, &'static str) {
    match permission {
        "process_management" => (
            "Process Management",
            "This allows NEURAX to adjust process priorities and manage background tasks to optimize system performance."
        ),
        "memory_optimization" => (
            "Memory Optimization", 
            "This allows NEURAX to clear system caches and optimize memory usage when your system is running low."
        ),
        "network_diagnostics" => (
            "Network Diagnostics",
            "This allows NEURAX to run network diagnostic tools and analyze connectivity issues with bootnodes."
        ),
        "auto_fix" => (
            "Automatic Fixes",
            "This allows NEURAX to automatically apply fixes for detected issues without asking for confirmation each time."
        ),
        "system_monitoring" => (
            "System Monitoring",
            "This allows NEURAX to access detailed system metrics including CPU, memory, GPU, and disk usage."
        ),
        _ => (
            "Unknown Permission",
            "This permission's purpose is not documented."
        ),
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================

#[tauri::command]
pub async fn neurax_check_admin() -> Result<PrivilegeCheckResult, String> {
    Ok(check_admin_privileges())
}

#[tauri::command]
pub async fn neurax_request_elevation(
    permission: String,
) -> Result<ElevationResult, String> {
    let (name, reason) = get_permission_description(&permission);
    request_elevation(name, reason).await.pipe(Ok)
}

#[tauri::command]
pub async fn neurax_get_permission_info(
    permission: String,
) -> Result<(String, String), String> {
    let (name, reason) = get_permission_description(&permission);
    Ok((name.to_string(), reason.to_string()))
}

// Helper trait for Result piping
trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}

impl<T> Pipe for T {}
