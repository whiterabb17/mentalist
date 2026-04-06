use anyhow::{bail, Result};
use async_trait::async_trait;
use bollard::container::LogOutput;
use bollard::models::{ContainerCreateBody as Config, HostConfig};
use bollard::query_parameters::{CreateImageOptions, LogsOptions, RemoveContainerOptions};
use bollard::Docker;
use futures_util::stream::StreamExt;
use mem_core::ToolDefinition;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::middleware::Middleware;
use crate::ToolCall;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Transient failure (timeout/network): {0}")]
    Transient(String),
    #[error("Tool not found: {0}")]
    NotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),
    #[error("Configuration error in executor: {0}")]
    ConfigError(String),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
    #[error("Containerization error: {0}")]
    SandboxError(String),
}

fn bytes_to_string_safe(bytes: &[u8]) -> String {
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            tracing::warn!("Tool produced invalid UTF-8 output. Using lossy conversion.");
            format!("[NON-UTF8 OUTPUT]: {}", String::from_utf8_lossy(bytes))
        }
    }
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Executes a tool by name with specified JSON arguments.
    async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String>;

    /// Lists all tools currently supported by this executor.
    async fn list_tools(&self) -> Result<Vec<ToolDefinition>>;

    /// Returns the current operational status of the executor.
    fn status(&self) -> String {
        "Ready".to_string()
    }
}

/// Optional command validator to block malicious patterns.
pub struct CommandValidator {
    pub allowed_cmds: Vec<String>,
    pub max_execution_time: std::time::Duration,
    pub max_memory_mb: u64,
}

impl CommandValidator {
    pub fn new_default() -> Self {
        Self {
            allowed_cmds: vec![
                "python".to_string(),
                "node".to_string(),
                "ruby".to_string(),
                "bash".to_string(),
                "sh".to_string(),
                "cat".to_string(),
                "ls".to_string(),
                "grep".to_string(),
                "jq".to_string(),
                "curl".to_string(),
                "wget".to_string(),
                "tar".to_string(),
                "zip".to_string(),
                "find".to_string(),
                "echo".to_string(),
            ],
            max_execution_time: std::time::Duration::from_secs(60),
            max_memory_mb: 512,
        }
    }

    pub fn from_config(config: &crate::config::SecurityConfig) -> Self {
        Self {
            allowed_cmds: config.allowed_commands.clone(),
            max_execution_time: std::time::Duration::from_secs(config.max_execution_time_seconds),
            max_memory_mb: config.max_memory_mb,
        }
    }

    pub fn validate(&self, cmd: &str, args: &[String], root_dir: &Path) -> Result<()> {
        // 1. Whitelist approach
        if !self.allowed_cmds.contains(&cmd.to_string()) {
            bail!("Command '{}' is not in the whitelist of allowed tools", cmd);
        }

        // 2. Validate all arguments for shell metacharacters
        for arg in args {
            if arg.contains(|c: char| ";&|`$()[]{}\"'\\".contains(c)) {
                tracing::error!(arg, "Dangerous shell characters detected");
                return Err(ToolError::SecurityViolation(format!("Dangerous characters in argument: {}", arg)).into());
            }

            // 3. Path expansion/traversal check
            if arg.contains("..") {
                tracing::error!(arg, "Path traversal detected");
                return Err(ToolError::SecurityViolation(format!("Path traversal attempt: {}", arg)).into());
            }

            if arg.starts_with('/') {
                let path = Path::new(arg);
                // Allow absolute paths ONLY if they are children of root_dir
                if !path.starts_with(root_dir) {
                    bail!(
                        "Access Denied: Absolute path '{}' is outside the sandbox root '{:?}'",
                        arg,
                        root_dir
                    );
                }
            }

            if arg.starts_with('~') {
                bail!(
                    "Access Denied: Home directory expansion (~) is not allowed in sandbox: {}",
                    arg
                );
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
        cpu_quota: Option<i64>,    // percentage * 1000
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
    pub middlewares: Vec<Arc<dyn Middleware>>,
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
            }
            ExecutionMode::Wasm {
                module_path: Some(path),
                ..
            } if !path.exists() => {
                bail!("Wasm module not found: {:?}", path);
            }
            _ => {}
        }

        Ok(Self {
            mode,
            root_dir,
            vault_dir,
            validator: CommandValidator::new_default(),
            middlewares: Vec::new(),
        })
    }

    pub fn add_middleware(&mut self, middleware: Arc<dyn Middleware>) {
        self.middlewares.push(middleware);
        self.middlewares.sort_by_key(|mw| mw.priority());
    }

    pub fn with_middleware(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.add_middleware(middleware);
        self
    }
}

#[async_trait]
impl ToolExecutor for SandboxedExecutor {
    async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let mut tool_call = ToolCall {
            name: name.to_string(),
            arguments: args.clone(),
        };

        // Run before_tool_call hooks (Safety Gates)
        for mw in &self.middlewares {
            mw.before_tool_call(&mut tool_call).await?;
        }

        let args_vec: Vec<String> = if let Some(obj) = tool_call.arguments.as_object() {
            obj.values()
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => String::new(),
                    other => serde_json::to_string(other).unwrap_or_default(),
                })
                .collect()
        } else if let Some(arr) = tool_call.arguments.as_array() {
            arr.iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => serde_json::to_string(other).unwrap_or_default(),
                })
                .collect()
        } else {
            vec![]
        };

        self.validator.validate(&tool_call.name, &args_vec, &self.root_dir)?;

        // If vault is set, we use it as the working directory / mount point for writes
        let working_dir = self.vault_dir.as_ref().unwrap_or(&self.root_dir);

        let output = match &self.mode {
            ExecutionMode::Local => self.execute_local(&tool_call.name, args_vec, working_dir),
            ExecutionMode::Docker {
                image,
                memory_limit,
                cpu_quota,
            } => {
                self.execute_docker(
                    image,
                    &tool_call.name,
                    args_vec,
                    working_dir,
                    *memory_limit,
                    *cpu_quota,
                )
                .await
            }
            ExecutionMode::Wasm {
                module_path,
                mount_root,
                env_vars,
            } => {
                let mut wasm_args = vec!["tool_runtime".to_string(), tool_call.name.clone()];
                wasm_args.extend(args_vec);
                self.execute_wasm(
                    module_path.as_ref(),
                    *mount_root,
                    wasm_args,
                    working_dir,
                    env_vars,
                )
                .await
            }
        };

        // Run after_tool_call hooks
        let mut final_output = output?;
        for mw in &self.middlewares {
            mw.after_tool_call(&tool_call, &mut final_output).await?;
        }

        Ok(final_output)
    }

    async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let mut tools = Vec::new();
        for cmd in &self.validator.allowed_cmds {
            let description = match cmd.as_str() {
                "ls" => "List directory contents.",
                "cat" => "Read and display file content.",
                "grep" => "Search for patterns in files.",
                "find" => "Search for files in a directory hierarchy.",
                "python" | "python3" => "Execute a Python script or command.",
                "node" => "Execute a Node.js script or command.",
                "bash" | "sh" => "Execute a shell script or command.",
                "curl" | "wget" => "Transfer data from or to a server.",
                "tar" | "zip" => "Archive or compress files.",
                "jq" => "Process and filter JSON data.",
                "echo" => "Display a line of text.",
                _ => "Execute a system command.",
            };

            tools.push(ToolDefinition {
                name: cmd.clone(),
                description: description.to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "args": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Arguments to pass to the command."
                        }
                    },
                    "required": ["args"]
                }),
            });
        }
        Ok(tools)
    }

    fn status(&self) -> String {
        format!("Active (Sandbox: {:?})", self.mode)
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
        let essential_vars = [
            "PATH",
            "SYSTEMROOT",
            "SYSTEMDRIVE",
            "TEMP",
            "TMP",
            "USERPROFILE",
        ];
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
                        let err = bytes_to_string_safe(&output.stderr);
                        return Err(ToolError::ExecutionFailed(format!("{} -> {}", attempt_cmd, err)).into());
                    }
                    return Ok(bytes_to_string_safe(&output.stdout));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    last_error = Some(e);
                    continue; // Try next fallback
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    return Err(ToolError::PermissionDenied(attempt_cmd).into());
                }
                Err(e) => {
                    return Err(ToolError::ExecutionFailed(format!("Critical failure ({}): {}", attempt_cmd, e)).into());
                }
            }
        }

        Err(ToolError::NotFound(format!("program not found (last tried: {}). Original error: {:?}", cmd, last_error)).into())
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
        use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
        use wasmtime_wasi::preview1::{self, WasiP1Ctx};
        use wasmtime_wasi::{DirPerms, FilePerms, I32Exit, WasiCtxBuilder};

        // 1. Engine Hardening: 4GB memory, Fuel enabled
        let mut config = wasmtime::Config::new();
        config
            .async_support(true)
            .consume_fuel(true)
            .max_wasm_stack(1024 * 1024); // 1MB stack

        let engine = Engine::new(&config)?;

        let module = if let Some(path) = module_path {
            Module::from_file(&engine, path)?
        } else {
            #[cfg(feature = "wasm-tools")]
            {
                Module::from_binary(&engine, DEFAULT_WASM)?
            }
            #[cfg(not(feature = "wasm-tools"))]
            {
                bail!("No Wasm module provided and wasm-tools feature is disabled");
            }
        };

        let mut linker: Linker<State> = Linker::new(&engine);
        preview1::add_to_linker_async(&mut linker, |s| &mut s.wasi)?;

        let stdout = MemoryOutputPipe::new(4096 * 4096); // 16MB cap
        let stderr = stdout.clone();

        let mut builder = WasiCtxBuilder::new();
        builder
            .stdout(stdout.clone())
            .stderr(stderr.clone())
            .args(&args);

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
            fn memory_growing(
                &mut self,
                _current: usize,
                desired: usize,
                _maximum: Option<usize>,
            ) -> Result<bool> {
                Ok(desired <= self.max_memory)
            }
            fn table_growing(
                &mut self,
                _current: usize,
                desired: usize,
                _maximum: Option<usize>,
            ) -> Result<bool> {
                Ok(desired <= 1000)
            }
            fn instances(&self) -> usize {
                1
            }
            fn tables(&self) -> usize {
                1
            }
            fn memories(&self) -> usize {
                1
            }
        }

        struct State {
            wasi: WasiP1Ctx,
            limits: Limits,
        }

        let mut store = Store::new(
            &engine,
            State {
                wasi,
                limits: Limits {
                    max_memory: 4 * 1024 * 1024 * 1024,
                }, // 4GB
            },
        );
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

        Ok(bytes_to_string_safe(&stdout.contents()))
    }

    async fn execute_docker(
        &self,
        image: &str,
        cmd: &str,
        args: Vec<String>,
        working_dir: &Path,
        memory_limit: Option<i64>,
        cpu_quota: Option<i64>,
    ) -> Result<String> {
        let docker = Docker::connect_with_local_defaults()?;

        let mut pull_stream = docker.create_image(
            Some(CreateImageOptions {
                from_image: Some(image.to_string()),
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(res) = pull_stream.next().await {
            res?;
        }

        let abs_root = working_dir.canonicalize()?.to_string_lossy().to_string();
        let binds = vec![format!("{}:/sandbox:rw", abs_root)];

        let host_config = HostConfig {
            binds: Some(binds),
            memory: memory_limit.or(Some(4 * 1024 * 1024 * 1024)), // Default 4GB
            cpu_quota: cpu_quota.or(Some(50000)),                  // Default 50%
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

        let container = docker.create_container(None, config).await?;

        // 3. Docker Resource Validation: Verify limits are applied
        let inspected = docker.inspect_container(&container.id, None).await?;
        if let Some(host_config) = inspected.host_config {
            if let (Some(limit), Some(config_limit)) = (host_config.memory, memory_limit) {
                if limit != config_limit {
                    tracing::warn!("Docker container {} memory limit mismatch: expected {}, got {}", container.id, config_limit, limit);
                }
            }
        }

        let result = async {
            docker.start_container(&container.id, None).await?;
            let mut logs = docker.logs(
                &container.id,
                Some(LogsOptions {
                    stdout: true,
                    stderr: true,
                    follow: true,
                    ..Default::default()
                }),
            );

            let mut output_bytes = Vec::new();
            while let Some(log_result) = logs.next().await {
                if let LogOutput::StdOut { message } | LogOutput::StdErr { message } = log_result? {
                    output_bytes.extend_from_slice(&message);
                }
            }
            Ok::<String, anyhow::Error>(bytes_to_string_safe(&output_bytes))
        }
        .await;

        // Ensure cleanup
        match docker.remove_container(&container.id, None).await {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Failed to clean up container {}: {}", container.id, e);
                // Attempt force removal
                let _ = docker
                    .remove_container(
                        &container.id,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
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

                match self.recursive_copy(vault, &self.root_dir).await {
                    Ok(_) => {
                        // Secure cleanup only on full success
                        tokio::fs::remove_dir_all(vault).await?;
                        tokio::fs::create_dir_all(vault).await?;
                        tracing::info!("Vault committed successfully and cleaned.");
                    }
                    Err(e) => {
                        tracing::error!("Vault commit failed during recursive copy: {}. Vault contents preserved.", e);
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn recursive_copy(&self, src: &Path, dst: &Path) -> Result<()> {
        let mut entries = tokio::fs::read_dir(src).await?;
        let mut total_size = 0u64;
        const MAX_VAULT_SIZE: u64 = 1024 * 1024 * 100; // 100MB max

        while let Some(entry) = entries.next_entry().await? {
            let src_path = entry.path();
            let dest_path = dst.join(entry.file_name());
            
            // Get file type directly from directory entry
            let ty = entry.file_type().await?;
            
            // Security: Always avoid symlinks in vault copy to prevent traversal
            if ty.is_symlink() {
                tracing::warn!("Skipping symlink to prevent traversal: {:?}", src_path);
                continue;
            }

            if ty.is_dir() {
                Box::pin(self.recursive_copy(&src_path, &dest_path)).await?;
            } else if ty.is_file() {
                // TOCTOU Fix: Open file and verify it's still a regular file before copying
                let mut src_file = tokio::fs::File::open(&src_path).await?;
                let metadata = src_file.metadata().await?;
                total_size += metadata.len();
                
                if total_size > MAX_VAULT_SIZE {
                    bail!("Vault copy exceeds size limit ({}MB)", MAX_VAULT_SIZE / 1024 / 1024);
                }

                if !metadata.is_file() {
                    bail!("Security violation: File type changed during vault copy at {:?}", src_path);
                }

                let mut dest_file = tokio::fs::File::create(&dest_path).await?;
                tokio::io::copy(&mut src_file, &mut dest_file).await?;
            }
        }
        Ok(())
    }
}

/// An entry in the MultiExecutor tracking its lifecycle state.
pub struct NamedExecutor {
    pub name: String,
    pub executor: Arc<dyn ToolExecutor>,
    pub enabled: bool,
}

/// Orchestrates multiple ToolExecutors.
pub struct MultiExecutor {
    pub executors: Mutex<Vec<NamedExecutor>>,
}

impl Default for MultiExecutor {
    fn default() -> Self {
        Self { executors: Mutex::new(Vec::new()) }
    }
}

impl MultiExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn add_executor(&self, name: String, executor: Arc<dyn ToolExecutor>) {
        let mut guard = self.executors.lock().await;
        if guard.iter().any(|e| e.name == name) {
            tracing::warn!("Executor with name '{}' already exists. Overwriting registration.", name);
            guard.retain(|e| e.name != name);
        }
        guard.push(NamedExecutor {
            name,
            executor,
            enabled: true,
        });
    }

    pub async fn set_executor_enabled(&self, name: &str, enabled: bool) -> bool {
        let mut guard = self.executors.lock().await;
        if let Some(exec) = guard.iter_mut().find(|e| e.name == name) {
            exec.enabled = enabled;
            true
        } else {
            false
        }
    }

    pub async fn list_executors(&self) -> Vec<(String, bool, String)> {
        let guard = self.executors.lock().await;
        guard.iter()
            .map(|e| (e.name.clone(), e.enabled, e.executor.status()))
            .collect()
    }
}

#[async_trait]

impl ToolExecutor for MultiExecutor {
    async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let guard = self.executors.lock().await;

        // 1. Explicit Routing: Try executors that definitively claim this tool
        for exec_entry in guard.iter() {
            if !exec_entry.enabled { continue; }
            if let Ok(tools) = exec_entry.executor.list_tools().await {
                if tools.iter().any(|t| t.name == name) {
                    tracing::debug!("Routing tool '{}' to executor '{}'", name, exec_entry.name);
                    return exec_entry.executor.execute(name, args).await;
                }
            }
        }

        // 2. Fallback Routing: Try any enabled executor (compatibility)
        for exec_entry in guard.iter() {
            if !exec_entry.enabled { continue; }
            match exec_entry.executor.execute(name, args.clone()).await {
                Ok(res) => return Ok(res),
                Err(_) => continue,
            }
        }

        Err(anyhow::anyhow!("Tool '{}' not found in any active executor", name))
    }

    async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let mut all_tools = Vec::new();
        let guard = self.executors.lock().await;
        for exec_entry in &*guard {
            if !exec_entry.enabled { continue; }
            match exec_entry.executor.list_tools().await {
                Ok(tools) => all_tools.extend(tools),
                Err(e) => {
                    tracing::warn!(executor = %exec_entry.name, error = %e, "Failed to list tools from executor");
                }
            }
        }
        Ok(all_tools)
    }

    fn status(&self) -> String {
        if let Ok(guard) = self.executors.try_lock() {
            let enabled_count = guard.iter().filter(|e| e.enabled).count();
            format!("{} executors active", enabled_count)
        } else {
            "Status currently unavailable (locked)".to_string()
        }
    }
}

/// Helper for dynamically loading executors from configuration.
pub struct DynamicExecutorLoader;

impl DynamicExecutorLoader {
    /// Instantiates a MultiExecutor populated with executors defined in the config.
    #[tracing::instrument(skip(config))]
    pub async fn load_from_config(config: &crate::config::ExecutorConfig, security: &crate::config::SecurityConfig) -> Result<Arc<MultiExecutor>> {
        let multi = Arc::new(MultiExecutor::new());
        let validator = CommandValidator::from_config(security);

        // 1. Create default based on mode
        let default_mode = match config.default_mode.to_lowercase().as_str() {
            "docker" => {
                ExecutionMode::Docker {
                    image: config.docker_image.clone().unwrap_or_else(|| "python:3.11-slim".into()),
                    memory_limit: Some((security.max_memory_mb * 1024 * 1024) as i64),
                    cpu_quota: Some(50000), // 50%
                }
            }
            "wasm" => {
                ExecutionMode::Wasm {
                    module_path: config.wasm_module_path.clone(),
                    mount_root: true,
                    env_vars: HashMap::new(),
                }
            }
            _ => ExecutionMode::Local,
        };

        let executor = SandboxedExecutor {
            mode: default_mode,
            root_dir: config.sandbox_root.clone(),
            vault_dir: config.vault_dir.clone(),
            validator,
            middlewares: Vec::new(),
        };

        multi.add_executor("default".to_string(), Arc::new(executor)).await;
        
        Ok(multi)
    }
}
