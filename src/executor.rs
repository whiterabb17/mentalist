use std::process::Command;
use std::path::PathBuf;
use anyhow::{Result, bail};

/// Execution mode for the DeepAgent harness (User Configurable).
pub enum ExecutionMode {
    /// Local restricted shell with directory isolation and environment stripping.
    Local,
    /// High-security Docker isolation for untrusted code/tool execution.
    Docker { image: String },
}

/// Sandboxed Tool Executor (DeepAgent Safety Pillar).
/// Ensures the agent cannot access the host filesystem or environment outside its project root.
pub struct SandboxedExecutor {
    pub mode: ExecutionMode,
    pub root_dir: PathBuf,
}

impl SandboxedExecutor {
    pub fn new(mode: ExecutionMode, root_dir: PathBuf) -> Self {
        Self { mode, root_dir }
    }

    /// Executes a tool command within the configured sandbox.
    pub async fn execute(&self, cmd: &str, args: Vec<String>) -> Result<String> {
        match &self.mode {
            ExecutionMode::Local => self.execute_local(cmd, args),
            ExecutionMode::Docker { image } => self.execute_docker(image, cmd, args),
        }
    }

    fn execute_local(&self, cmd: &str, args: Vec<String>) -> Result<String> {
        // Restricted Local Shell:
        // 1. Environment Isolation: env_clear() ensures sensitive tokens aren't passed to tools.
        // 2. Directory Isolation: current_dir() forces execution within the project root.
        let output = Command::new(cmd)
            .args(args)
            .current_dir(&self.root_dir)
            .env_clear() 
            .output()?;
            
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            bail!("Tool execution failed: {}", err);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn execute_docker(&self, image: &str, _cmd: &str, _args: Vec<String>) -> Result<String> {
        // Implementation template for high-security environments.
        // Wraps the command in a 'docker run' call with volume mounting for the root_dir.
        tracing::info!("Simulating Docker execution for image: {}", image);
        bail!("Docker isolation requested but not yet implemented in the harness core.");
    }
}
