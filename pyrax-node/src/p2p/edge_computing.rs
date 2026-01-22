//! Edge Computing Module - ARM64/Embedded/Light Mode Support
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeMode {
    #[default]
    Full,
    Light,
    Embedded,
    Wasm,
}

impl NodeMode {
    pub fn max_peers(&self) -> usize {
        match self { Self::Full => 100, Self::Light => 25, Self::Embedded => 10, Self::Wasm => 5 }
    }
    pub fn cache_mb(&self) -> usize {
        match self { Self::Full => 512, Self::Light => 128, Self::Embedded => 32, Self::Wasm => 8 }
    }
    pub fn sync_headers_only(&self) -> bool {
        matches!(self, Self::Light | Self::Wasm)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture { X86_64, Arm64, Arm32, Wasm32, Unknown }

impl Architecture {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")] { return Self::X86_64; }
        #[cfg(target_arch = "aarch64")] { return Self::Arm64; }
        #[cfg(target_arch = "arm")] { return Self::Arm32; }
        #[cfg(target_arch = "wasm32")] { return Self::Wasm32; }
        #[allow(unreachable_code)] Self::Unknown
    }
    pub fn recommended_mode(&self) -> NodeMode {
        match self {
            Self::X86_64 => NodeMode::Full,
            Self::Arm64 => NodeMode::Light,
            Self::Arm32 | Self::Wasm32 => NodeMode::Embedded,
            Self::Unknown => NodeMode::Light,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EdgeConfig {
    pub mode: NodeMode,
    pub arch: Architecture,
    pub max_memory_mb: usize,
    pub storage_path: String,
    pub sync_interval: Duration,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        let arch = Architecture::detect();
        Self {
            mode: arch.recommended_mode(),
            arch,
            max_memory_mb: 256,
            storage_path: "/var/lib/pyrax".to_string(),
            sync_interval: Duration::from_secs(30),
        }
    }
}

pub struct EdgeNode {
    pub config: EdgeConfig,
    pub headers_only: bool,
}

impl EdgeNode {
    pub fn new(config: EdgeConfig) -> Self {
        let headers_only = config.mode.sync_headers_only();
        Self { config, headers_only }
    }
}

impl Default for EdgeNode { fn default() -> Self { Self::new(EdgeConfig::default()) } }
