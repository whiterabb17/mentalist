use anyhow::{bail, Context, Result};
use bollard::container::{Config, LogOutput};
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::collections::HashMap;

/// Optional command validator to block malicious patterns.
pub struct CommandValidator {
    pub blacklisted_cmds: Vec<String>,
}

impl CommandValidator {
    pub fn new_default() -> Self {
        Self {
            blacklisted_cmds: vec![
                "rm".to_string(), "mkfs".to_string(), "dd".to_string(), 
                "mv".to_string(), "cp".to_string(), "chmod".to_string()
            ],
        }
    }

    pub fn validate(&self, cmd: &str, _args: &[String]) -> Result<()> {
        if self.blacklisted_cmds.contains(&cmd.to_string()) {
            bail!("Command '{}' is blacklisted for security reasons", cmd);
        }
        Ok(())
    }
}

/// Execution mode for the DeepAgent harness.
#[derive(Debug, Clone)]
pub enum ExecutionMode {
    Local,
    Docker { 
        image: String,
        memory_limit: Option<i64>, // bytes
        cpu_quota: Option<i64>, // percentage * 1000
    },
    Wasm {
        module_path: Option<PathBuf>,
        mount_root: bool,
        env_vars: HashMap<String, String>,
    },
}

#[cfg(feature = "wasm-tools")]
const DEFAULT_WASM: &[u8] = include_bytes!("resources/tool_runner.wasm");

pub struct SandboxedExecutor {
    pub mode: ExecutionMode,
    pub root_dir: PathBuf,
    pub vault_dir: Option<PathBuf>,
    pub validator: CommandValidator,
}

impl SandboxedExecutor {
    pub fn new(mode: ExecutionMode, root_dir: PathBuf, vault_dir: Option<PathBuf>) -> Self {
        Self { 
            mode, 
            root_dir, 
            vault_dir,
            validator: CommandValidator::new_default(),
        }
    }

    pub async fn execute(&self, cmd: &str, args: Vec<String>) -> Result<String> {
        self.validator.validate(cmd, &args)?;

        // If vault is set, we use it as the working directory / mount point for writes
        let working_dir = self.vault_dir.as_ref().unwrap_or(&self.root_dir);

        match &self.mode {
            ExecutionMode::Local => self.execute_local(cmd, args, working_dir),
            ExecutionMode::Docker { image, memory_limit, cpu_quota } => {
                self.execute_docker(image, cmd, args, working_dir, *memory_limit, *cpu_quota).await
            },
            ExecutionMode::Wasm { module_path, mount_root, env_vars } => {
                self.execute_wasm(module_path.as_ref(), *mount_root, args, working_dir, env_vars).await
            },
        }
    }

    fn execute_local(&self, cmd: &str, args: Vec<String>, working_dir: &Path) -> Result<String> {
        let output = Command::new(cmd)
            .args(args)
            .current_dir(working_dir)
            .env_clear()
            .output()
            .context("Local tool execution failed")?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            bail!("Tool execution failed: {}", err);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
        
        // Pull logic...
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
        docker.start_container::<String>(&container.id, None).await?;

        let mut logs = docker.logs(&container.id, Some(bollard::container::LogsOptions::<String> { stdout: true, stderr: true, follow: true, ..Default::default() }));

        let mut output = String::new();
        while let Some(log_result) = logs.next().await {
            if let LogOutput::StdOut { message } | LogOutput::StdErr { message } = log_result? {
                output.push_str(&String::from_utf8_lossy(&message));
            }
        }

        let _ = docker.remove_container(&container.id, None).await;
        Ok(output)
    }

    /// Commit staged changes from vault to project root.
    pub async fn commit_vault(&self) -> Result<()> {
        if let Some(vault) = &self.vault_dir {
            if vault.exists() {
                self.recursive_copy(vault, &self.root_dir).await?;
                // Optionally clear vault after commit
                tokio::fs::remove_dir_all(vault).await?;
                tokio::fs::create_dir_all(vault).await?;
            }
        }
        Ok(())
    }

    async fn recursive_copy(&self, src: &Path, dst: &Path) -> Result<()> {
        tokio::fs::create_dir_all(dst).await?;
        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let ty = entry.file_type().await?;
            if ty.is_dir() {
                Box::pin(self.recursive_copy(&entry.path(), &dst.join(entry.file_name()))).await?;
            } else {
                tokio::fs::copy(entry.path(), dst.join(entry.file_name())).await?;
            }
        }
        Ok(())
    }
}
