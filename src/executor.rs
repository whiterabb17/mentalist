use anyhow::{bail, Result};
use bollard::container::{Config, LogOutput};
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::collections::HashMap;
use std::sync::Arc;
use mem_core::ToolDefinition;
use async_trait::async_trait;

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Executes a tool by name with specified JSON arguments.
    async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String>;
    
    /// Lists all tools currently supported by this executor.
    async fn list_tools(&self) -> Result<Vec<ToolDefinition>>;
}

/// Optional command validator to block malicious patterns.
pub struct CommandValidator {
    pub allowed_cmds: Vec<String>,
}

impl CommandValidator {
    pub fn new_default() -> Self {
        Self {
            allowed_cmds: vec![
                "python".to_string(), "node".to_string(), "ruby".to_string(), 
                "bash".to_string(), "sh".to_string(), "cat".to_string(), 
                "ls".to_string(), "grep".to_string(), "jq".to_string(), 
                "curl".to_string(), "wget".to_string(), "tar".to_string(), 
                "zip".to_string(), "find".to_string(), "echo".to_string()
            ],
        }
    }

    pub fn validate(&self, cmd: &str, args: &[String]) -> Result<()> {
        // 1. Whitelist approach
        if !self.allowed_cmds.contains(&cmd.to_string()) {
            bail!("Command '{}' is not in the whitelist of allowed tools", cmd);
        }

        // 2. Validate all arguments for shell metacharacters
        for arg in args {
            if arg.contains(|c: char| ";&|`$()[]{}\"'\\".contains(c)) {
                bail!("Argument contains potentially dangerous shell characters: {}", arg);
            }
            
            // 3. Path traversal check
            if arg.contains("..") || arg.starts_with('/') {
                bail!("Argument appears to be a path traversal attempt or absolute path: {}", arg);
            }
        }

        Ok(())
    }
}

/// Execution mode for the DeepAgent harness, determining how tools are ran.
#[derive(Debug, Clone)]
pub enum ExecutionMode {
    /// Tools are executed as local processes. Recommended for trusted environments only.
    Local,
    /// Tools are executed inside a Docker container. Provides host OS isolation.
    Docker { 
        image: String,
        memory_limit: Option<i64>, // bytes
        cpu_quota: Option<i64>, // percentage * 1000
    },
    /// Tools are executed as Wasm modules using `wasmtime`. Provides the highest level of isolation.
    Wasm {
        /// Optional path to a specific Wasm module. If None, a default tool runner may be used.
        module_path: Option<PathBuf>,
        /// Whether to mount the sandbox root directory to `/sandbox` in the Wasm instance.
        mount_root: bool,
        /// Explicit environment variables to provide to the Wasm module.
        env_vars: HashMap<String, String>,
    },
}

#[cfg(feature = "wasm-tools")]
const DEFAULT_WASM: &[u8] = include_bytes!("resources/tool_runner.wasm");

/// A security-hardened tool executor that supports various sandboxing levels.
///
/// `SandboxedExecutor` validates commands against a whitelist and ensures they do not
/// perform forbidden operations like path traversal before executing them in the chosen `ExecutionMode`.
pub struct SandboxedExecutor {
    pub mode: ExecutionMode,
    /// The base directory for file operations.
    pub root_dir: PathBuf,
    /// An optional staging area for file writes before they are "committed" to root_dir.
    pub vault_dir: Option<PathBuf>,
    /// The validator used to check commands and arguments for safety.
    pub validator: CommandValidator,
}

impl SandboxedExecutor {
    pub fn new(mode: ExecutionMode, root_dir: PathBuf, vault_dir: Option<PathBuf>) -> Result<Self> {
        // Validate root_dir
        if !root_dir.exists() {
            bail!("Root directory does not exist: {:?}", root_dir);
        }
        if !root_dir.is_dir() {
            bail!("Root path is not a directory: {:?}", root_dir);
        }

        // Validate vault_dir if provided
        if let Some(ref vault) = vault_dir {
            if !vault.exists() {
                bail!("Vault directory does not exist: {:?}", vault);
            }
        }

        // Validate execution mode environment
        match &mode {
            ExecutionMode::Docker { .. } => {
                // Warning only for docker daemon availability
                if let Err(e) = std::process::Command::new("docker").arg("version").output() {
                    tracing::warn!("Docker daemon validation warning: {}", e);
                }
            },
            ExecutionMode::Wasm { module_path: Some(path), .. } => {
                if !path.exists() {
                    bail!("Wasm module not found: {:?}", path);
                }
            },
            _ => {},
        }

        Ok(Self { 
            mode, 
            root_dir, 
            vault_dir,
            validator: CommandValidator::new_default(),
        })
    }
}

#[async_trait]
impl ToolExecutor for SandboxedExecutor {
    async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let args_vec: Vec<String> = if let Some(obj) = args.as_object() {
            obj.values().map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => String::new(),
                other => serde_json::to_string(other).unwrap_or_default(),
            }).collect()
        } else if let Some(arr) = args.as_array() {
            arr.iter().map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string(other).unwrap_or_default(),
            }).collect()
        } else {
            vec![]
        };

        self.validator.validate(name, &args_vec)?;

        // If vault is set, we use it as the working directory / mount point for writes
        let working_dir = self.vault_dir.as_ref().unwrap_or(&self.root_dir);

        match &self.mode {
            ExecutionMode::Local => self.execute_local(name, args_vec, working_dir),
            ExecutionMode::Docker { image, memory_limit, cpu_quota } => {
                self.execute_docker(image, name, args_vec, working_dir, *memory_limit, *cpu_quota).await
            },
            ExecutionMode::Wasm { module_path, mount_root, env_vars } => {
                let mut wasm_args = vec!["tool_runtime".to_string(), name.to_string()];
                wasm_args.extend(args_vec);
                self.execute_wasm(module_path.as_ref(), *mount_root, wasm_args, working_dir, env_vars).await
            },
        }
    }

    async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        // For now, SandboxedExecutor doesn't explicitly expose its tools for discovery 
        // as they are usually defined globally in the system prompt.
        Ok(vec![])
    }
}

impl SandboxedExecutor {

    fn execute_local(&self, cmd: &str, args: Vec<String>, working_dir: &Path) -> Result<String> {
        let mut commands_to_try = vec![cmd.to_string()];
        
        // Add common fallbacks for portability
        match cmd {
            "python" => commands_to_try.push("python3".to_string()),
            "python3" => commands_to_try.push("python".to_string()),
            "pip" => commands_to_try.push("pip3".to_string()),
            "pip3" => commands_to_try.push("pip".to_string()),
            "node" => commands_to_try.push("nodejs".to_string()),
            _ => {}
        }

        // Essential environment variables for tool initialization (e.g., Python on Windows)
        let mut safe_env = HashMap::new();
        let essential_vars = ["PATH", "SYSTEMROOT", "SYSTEMDRIVE", "TEMP", "TMP", "USERPROFILE"];
        for var in essential_vars {
            if let Ok(val) = std::env::var(var) {
                safe_env.insert(var.to_string(), val);
            }
        }

        let mut last_error = None;
        for attempt_cmd in commands_to_try {
            let result = Command::new(&attempt_cmd)
                .args(&args)
                .current_dir(working_dir)
                .env_clear()
                .envs(&safe_env)
                .output();

            match result {
                Ok(output) => {
                    if !output.status.success() {
                        let err = String::from_utf8_lossy(&output.stderr).to_string();
                        bail!("Tool execution failed ({}): {}", attempt_cmd, err);
                    }
                    return Ok(String::from_utf8_lossy(&output.stdout).to_string());
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    last_error = Some(e);
                    continue; // Try next fallback
                }
                Err(e) => {
                    bail!("Local tool execution critical failure ({}): {}", attempt_cmd, e);
                }
            }
        }

        Err(anyhow::anyhow!("Local tool execution failed: program not found (last tried: {}). Original error: {:?}", cmd, last_error))
    }

    async fn execute_wasm(
        &self,
        module_path: Option<&PathBuf>,
        mount_root: bool,
        args: Vec<String>,
        working_dir: &Path,
        env_vars: &HashMap<String, String>,
    ) -> Result<String> {
        use wasmtime::*;
        use wasmtime_wasi::pipe::MemoryOutputPipe;
        use wasmtime_wasi::preview1::{self, WasiP1Ctx};
        use wasmtime_wasi::{DirPerms, FilePerms, I32Exit, WasiCtxBuilder};

        // 1. Engine Hardening: 4GB memory, Fuel enabled
        let mut config = wasmtime::Config::new();
        config.async_support(true)
             .consume_fuel(true)
             .max_wasm_stack(1024 * 1024); // 1MB stack

        let engine = Engine::new(&config)?;
        
        let module = if let Some(path) = module_path {
            Module::from_file(&engine, path)?
        } else {
            #[cfg(feature = "wasm-tools")]
            { Module::from_binary(&engine, DEFAULT_WASM)? }
            #[cfg(not(feature = "wasm-tools"))]
            { bail!("No Wasm module provided and wasm-tools feature is disabled"); }
        };

        let mut linker: Linker<State> = Linker::new(&engine);
        preview1::add_to_linker_async(&mut linker, |s| &mut s.wasi)?;

        let stdout = MemoryOutputPipe::new(4096 * 4096); // 16MB cap
        let stderr = stdout.clone();

        let mut builder = WasiCtxBuilder::new();
        builder.stdout(stdout.clone()).stderr(stderr.clone()).args(&args);
        
        // Pass explicit env vars ONLY
        for (k, v) in env_vars {
            builder.env(k, v);
        }

        if mount_root {
            let abs_path = working_dir.to_string_lossy().to_string();
            builder.preopened_dir(abs_path, "/sandbox", DirPerms::all(), FilePerms::all())?;
        }

        let wasi = builder.build_p1();
        
        // 2. Resource Limiting
        struct Limits {
            max_memory: usize,
        }
        impl ResourceLimiter for Limits {
            fn memory_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> Result<bool> {
                Ok(desired <= self.max_memory)
            }
            fn table_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> Result<bool> {
                Ok(desired <= 1000)
            }
            fn instances(&self) -> usize { 1 }
            fn tables(&self) -> usize { 1 }
            fn memories(&self) -> usize { 1 }
        }

        struct State {
            wasi: WasiP1Ctx,
            limits: Limits,
        }

        let mut store = Store::new(&engine, State { 
            wasi, 
            limits: Limits { max_memory: 4 * 1024 * 1024 * 1024 } // 4GB 
        });
        store.set_fuel(50_000_000)?; // 50M instructions
        store.limiter(|s| &mut s.limits);

        let instance = linker.instantiate_async(&mut store, &module).await?;
        let func = instance.get_typed_func::<(), ()>(&mut store, "_start")?;

        if let Err(e) = func.call_async(&mut store, ()).await {
            if let Some(exit) = e.downcast_ref::<I32Exit>() {
                if exit.0 != 0 {
                    bail!("Wasm execution failed with exit code: {}", exit.0);
                }
            } else {
                bail!("Wasm execution failed: {:?}", e);
            }
        }

        Ok(String::from_utf8_lossy(&stdout.contents()).to_string())
    }

    async fn execute_docker(&self, image: &str, cmd: &str, args: Vec<String>, working_dir: &Path, memory_limit: Option<i64>, cpu_quota: Option<i64>) -> Result<String> {
        let docker = Docker::connect_with_local_defaults()?;
        
        let mut pull_stream = docker.create_image(Some(CreateImageOptions { from_image: image.to_string(), ..Default::default() }), None, None);
        while let Some(res) = pull_stream.next().await { res?; }

        let abs_root = working_dir.canonicalize()?.to_string_lossy().to_string();
        let binds = vec![format!("{}:/sandbox:rw", abs_root)];

        let host_config = HostConfig {
            binds: Some(binds),
            memory: memory_limit.or(Some(4 * 1024 * 1024 * 1024)), // Default 4GB
            cpu_quota: cpu_quota.or(Some(50000)), // Default 50%
            ..Default::default()
        };

        let mut full_cmd = vec![cmd.to_string()];
        full_cmd.extend(args);

        let config = Config {
            image: Some(image.to_string()),
            cmd: Some(full_cmd),
            working_dir: Some("/sandbox".to_string()),
            host_config: Some(host_config),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let container = docker.create_container::<String, String>(None, config).await?;
        
        let result = async {
            docker.start_container::<String>(&container.id, None).await?;
            let mut logs = docker.logs(&container.id, Some(bollard::container::LogsOptions::<String> { stdout: true, stderr: true, follow: true, ..Default::default() }));

            let mut output = String::new();
            while let Some(log_result) = logs.next().await {
                if let LogOutput::StdOut { message } | LogOutput::StdErr { message } = log_result? {
                    output.push_str(&String::from_utf8_lossy(&message));
                }
            }
            Ok::<String, anyhow::Error>(output)
        }.await;

        // Ensure cleanup
        match docker.remove_container(&container.id, None).await {
            Ok(_) => {},
            Err(e) => {
                tracing::error!("Failed to clean up container {}: {}", container.id, e);
                // Attempt force removal
                let _ = docker.remove_container(
                    &container.id, 
                    Some(bollard::container::RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    })
                ).await;
            }
        }

        result
    }

    /// Commit staged changes from vault to project root safely.
    pub async fn commit_vault(&self) -> Result<()> {
        if let Some(vault) = &self.vault_dir {
            if vault.exists() {
                // Validate vault is within project boundaries
                let vault_canonical = vault.canonicalize()?;
                let root_canonical = self.root_dir.canonicalize()?;
                
                if !vault_canonical.starts_with(&root_canonical) {
                    bail!(
                        "Security breach attempt: Vault directory {:?} is outside project root {:?}",
                        vault_canonical,
                        root_canonical
                    );
                }

                self.recursive_copy(vault, &self.root_dir).await?;
                
                // Secure cleanup with verification
                tokio::fs::remove_dir_all(vault).await?;
                tokio::fs::create_dir_all(vault).await?;
            }
        }
        Ok(())
    }

    async fn recursive_copy(&self, src: &Path, dst: &Path) -> Result<()> {
        let mut entries = tokio::fs::read_dir(src).await?;
        let mut total_size = 0u64;
        const MAX_VAULT_SIZE: u64 = 1024 * 1024 * 100; // 100MB max
        
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            total_size += metadata.len();
            
            if total_size > MAX_VAULT_SIZE {
                bail!("Vault copy exceeds size limit ({}MB)", MAX_VAULT_SIZE / 1024 / 1024);
            }
            
            let ty = entry.file_type().await?;
            if ty.is_symlink() {
                tracing::warn!("Skipping symlink to prevent traversal: {:?}", entry.path());
                continue;
            }

            if ty.is_dir() {
                Box::pin(self.recursive_copy(&entry.path(), &dst.join(entry.file_name()))).await?;
            } else {
                tokio::fs::copy(entry.path(), dst.join(entry.file_name())).await?;
            }
        }
        Ok(())
    }
}

/// Orchestrates multiple ToolExecutors.
pub struct MultiExecutor {
    pub executors: Vec<Arc<dyn ToolExecutor>>,
}

impl MultiExecutor {
    pub fn new() -> Self {
        Self { executors: Vec::new() }
    }

    pub fn add_executor(&mut self, executor: Arc<dyn ToolExecutor>) {
        self.executors.push(executor);
    }
}

#[async_trait]
impl ToolExecutor for MultiExecutor {
    async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let mut last_err = anyhow::anyhow!("No executor found for tool: {}", name);
        
        for exec in &self.executors {
            let tools = exec.list_tools().await?;
            if tools.iter().any(|t| t.name == name) {
                return exec.execute(name, args).await;
            }
        }
        
        for exec in &self.executors {
            match exec.execute(name, args.clone()).await {
                Ok(res) => return Ok(res),
                Err(e) => last_err = e,
            }
        }

        Err(last_err)
    }

    async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let mut all_tools = Vec::new();
        for exec in &self.executors {
            all_tools.extend(exec.list_tools().await?);
        }
        Ok(all_tools)
    }
}
