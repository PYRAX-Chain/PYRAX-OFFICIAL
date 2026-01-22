//! CLI self-update command
//! 
//! Allows users to update inferno-cli to the latest version or a specific version.

use anyhow::{Context, Result};
use clap::Args;
use colored::Colorize;
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const GITHUB_API_URL: &str = "https://api.github.com/repos/PYRAX-Chain/PYRAX-OFFICIAL/releases";
const GITHUB_RELEASES_URL: &str = "https://github.com/PYRAX-Chain/PYRAX-OFFICIAL/releases/download";

#[derive(Args)]
pub struct UpdateArgs {
    /// Update to the latest version
    #[arg(long, conflicts_with = "version")]
    pub latest: bool,

    /// Update to a specific version (e.g., 0.3.10)
    #[arg(long, short = 'V', conflicts_with = "latest")]
    pub version: Option<String>,

    /// Check for updates without installing
    #[arg(long, short)]
    pub check: bool,

    /// Force update even if already on target version
    #[arg(long, short)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: String,
    published_at: String,
    assets: Vec<GithubAsset>,
    prerelease: bool,
    draft: bool,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Get the current platform identifier for asset matching
fn get_platform_identifier() -> &'static str {
    match (env::consts::OS, env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("macos", "aarch64") => "darwin-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        _ => "unknown",
    }
}

/// Get the binary extension for the current platform
fn get_binary_extension() -> &'static str {
    if cfg!(windows) { ".exe" } else { "" }
}

/// Fetch releases from GitHub API
async fn fetch_releases(client: &Client) -> Result<Vec<GithubRelease>> {
    let url = format!("{}?per_page=20", GITHUB_API_URL);
    
    let response = client
        .get(&url)
        .header("User-Agent", "inferno-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .context("Failed to fetch releases from GitHub")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub API returned status: {}", response.status());
    }

    let releases: Vec<GithubRelease> = response
        .json()
        .await
        .context("Failed to parse GitHub releases")?;

    // Filter to only CLI releases (tag starts with cli-v)
    Ok(releases
        .into_iter()
        .filter(|r| r.tag_name.starts_with("cli-v") && !r.draft)
        .collect())
}

/// Get the latest stable release
async fn get_latest_release(client: &Client) -> Result<GithubRelease> {
    let releases = fetch_releases(client).await?;
    
    releases
        .into_iter()
        .find(|r| !r.prerelease)
        .ok_or_else(|| anyhow::anyhow!("No stable releases found"))
}

/// Get a specific release by version
async fn get_release_by_version(client: &Client, version: &str) -> Result<GithubRelease> {
    let releases = fetch_releases(client).await?;
    
    // Normalize version (remove 'v' prefix if present)
    let normalized = version.trim_start_matches('v');
    let tag = format!("cli-v{}", normalized);
    
    releases
        .into_iter()
        .find(|r| r.tag_name == tag || r.tag_name == format!("v{}", normalized))
        .ok_or_else(|| anyhow::anyhow!("Version {} not found", version))
}

/// Extract version number from tag (e.g., "cli-v0.3.10" -> "0.3.10")
fn extract_version(tag: &str) -> &str {
    tag.trim_start_matches("cli-v").trim_start_matches('v')
}

/// Compare versions (returns true if v1 < v2)
fn version_less_than(v1: &str, v2: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    
    let v1_parts = parse(v1);
    let v2_parts = parse(v2);
    
    for (a, b) in v1_parts.iter().zip(v2_parts.iter()) {
        if a < b { return true; }
        if a > b { return false; }
    }
    
    v1_parts.len() < v2_parts.len()
}

/// Find the appropriate asset for the current platform
fn find_platform_asset(release: &GithubRelease) -> Option<&GithubAsset> {
    let platform = get_platform_identifier();
    let ext = get_binary_extension();
    
    // Look for inferno-{platform} or inferno-cli-{platform}
    release.assets.iter().find(|a| {
        let name = a.name.to_lowercase();
        (name.contains("inferno") && name.contains(platform)) ||
        (name.starts_with("inferno-cli") && name.contains(platform)) ||
        (name.starts_with("inferno") && name.contains(platform) && 
         (name.ends_with(ext) || name.ends_with(".tar.gz") || name.ends_with(".zip")))
    })
}

/// Download and install the update
async fn download_and_install(client: &Client, asset: &GithubAsset) -> Result<()> {
    println!("  {} {}", "Downloading:".cyan(), asset.name);
    println!("  {} {:.2} MB", "Size:".dimmed(), asset.size as f64 / 1_000_000.0);
    
    // Download the binary
    let response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", "inferno-cli")
        .send()
        .await
        .context("Failed to download update")?;

    if !response.status().is_success() {
        anyhow::bail!("Download failed with status: {}", response.status());
    }

    let bytes = response.bytes().await.context("Failed to read download")?;
    
    // Get current executable path
    let current_exe = env::current_exe().context("Failed to get current executable path")?;
    let backup_path = current_exe.with_extension("old");
    
    println!("  {} Installing update...", "→".cyan());
    
    // Handle compressed archives
    if asset.name.ends_with(".tar.gz") {
        install_from_targz(&bytes, &current_exe, &backup_path)?;
    } else if asset.name.ends_with(".zip") {
        install_from_zip(&bytes, &current_exe, &backup_path)?;
    } else {
        // Direct binary
        install_binary(&bytes, &current_exe, &backup_path)?;
    }
    
    println!("  {} Update installed successfully!", "✓".green());
    
    // Clean up backup
    let _ = fs::remove_file(&backup_path);
    
    Ok(())
}

/// Install from tar.gz archive
fn install_from_targz(bytes: &[u8], current_exe: &PathBuf, backup_path: &PathBuf) -> Result<()> {
    use flate2::read::GzDecoder;
    use tar::Archive;
    use std::io::Cursor;
    
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(decoder);
    
    let temp_dir = env::temp_dir().join("inferno-update");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir)?;
    
    archive.unpack(&temp_dir)?;
    
    // Find the inferno binary in the extracted files
    let binary_name = if cfg!(windows) { "inferno.exe" } else { "inferno" };
    let mut found_binary = None;
    
    for entry in fs::read_dir(&temp_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.file_name().map(|n| n.to_string_lossy().contains("inferno")).unwrap_or(false) {
            found_binary = Some(path);
            break;
        }
        if path.is_dir() {
            // Check subdirectory
            for sub_entry in fs::read_dir(&path)? {
                let sub_entry = sub_entry?;
                let sub_path = sub_entry.path();
                if sub_path.file_name() == Some(std::ffi::OsStr::new(binary_name)) {
                    found_binary = Some(sub_path);
                    break;
                }
            }
        }
    }
    
    let new_binary = found_binary.ok_or_else(|| anyhow::anyhow!("Binary not found in archive"))?;
    
    // Backup and replace
    if current_exe.exists() {
        fs::rename(current_exe, backup_path)?;
    }
    fs::copy(&new_binary, current_exe)?;
    
    // Set executable permission on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(current_exe)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(current_exe, perms)?;
    }
    
    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
    
    Ok(())
}

/// Install from zip archive
fn install_from_zip(bytes: &[u8], current_exe: &PathBuf, backup_path: &PathBuf) -> Result<()> {
    use std::io::Cursor;
    use zip::ZipArchive;
    
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    
    let temp_dir = env::temp_dir().join("inferno-update");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir)?;
    
    archive.extract(&temp_dir)?;
    
    // Find the inferno binary
    let binary_name = if cfg!(windows) { "inferno.exe" } else { "inferno" };
    let mut found_binary = None;
    
    fn find_binary(dir: &PathBuf, name: &str) -> Option<PathBuf> {
        for entry in fs::read_dir(dir).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_file() && path.file_name()?.to_string_lossy().contains("inferno") {
                return Some(path);
            }
            if path.is_dir() {
                if let Some(p) = find_binary(&path, name) {
                    return Some(p);
                }
            }
        }
        None
    }
    
    found_binary = find_binary(&temp_dir, binary_name);
    
    let new_binary = found_binary.ok_or_else(|| anyhow::anyhow!("Binary not found in archive"))?;
    
    // Backup and replace
    if current_exe.exists() {
        fs::rename(current_exe, backup_path)?;
    }
    fs::copy(&new_binary, current_exe)?;
    
    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
    
    Ok(())
}

/// Install binary directly
fn install_binary(bytes: &[u8], current_exe: &PathBuf, backup_path: &PathBuf) -> Result<()> {
    // Backup current binary
    if current_exe.exists() {
        fs::rename(current_exe, backup_path)?;
    }
    
    // Write new binary
    let mut file = fs::File::create(current_exe)?;
    file.write_all(bytes)?;
    
    // Set executable permission on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(current_exe)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(current_exe, perms)?;
    }
    
    Ok(())
}

/// Main update command handler
pub async fn run(args: UpdateArgs) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let platform = get_platform_identifier();
    
    println!("\n{} {}", "🔥".bright_red(), "Inferno CLI Update".bright_white().bold());
    println!("  {} {}", "Current version:".dimmed(), current_version);
    println!("  {} {}", "Platform:".dimmed(), platform);
    println!();
    
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    
    // Get target release
    let release = if let Some(version) = &args.version {
        println!("  {} Fetching version {}...", "→".cyan(), version);
        get_release_by_version(&client, version).await?
    } else {
        println!("  {} Checking for latest version...", "→".cyan());
        get_latest_release(&client).await?
    };
    
    let target_version = extract_version(&release.tag_name);
    
    println!("  {} {}", "Target version:".dimmed(), target_version.green());
    println!("  {} {}", "Release:".dimmed(), release.name);
    
    // Check if update is needed
    if target_version == current_version && !args.force {
        println!("\n  {} Already on version {}", "✓".green(), current_version);
        return Ok(());
    }
    
    if version_less_than(target_version, current_version) && !args.force {
        println!("\n  {} Target version {} is older than current {}", 
            "⚠".yellow(), target_version, current_version);
        println!("  Use --force to downgrade");
        return Ok(());
    }
    
    // Check only mode
    if args.check {
        if version_less_than(current_version, target_version) {
            println!("\n  {} Update available: {} → {}", 
                "!".yellow(), current_version, target_version.green());
            println!("  Run `inferno update --latest` to update");
        } else {
            println!("\n  {} You're up to date!", "✓".green());
        }
        return Ok(());
    }
    
    // Find platform asset
    let asset = find_platform_asset(&release)
        .ok_or_else(|| anyhow::anyhow!(
            "No release asset found for platform: {}\nAvailable assets: {:?}",
            platform,
            release.assets.iter().map(|a| &a.name).collect::<Vec<_>>()
        ))?;
    
    println!();
    
    // Download and install
    download_and_install(&client, asset).await?;
    
    println!("\n  {} Updated from {} to {}", 
        "✓".green().bold(), 
        current_version.dimmed(), 
        target_version.green().bold());
    println!("  Restart your terminal to use the new version.\n");
    
    Ok(())
}
