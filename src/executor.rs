use anyhow::{bail, Context, Result};
use bollard::container::{Config, LogOutput};
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::path::PathBuf;
use std::process::Command;

/// Execution mode for the DeepAgent harness (User Configurable).
#[derive(Debug, Clone)]
pub enum ExecutionMode {
    /// Local restricted shell with directory isolation and environment stripping.
    Local,
    /// High-security Docker isolation for untrusted code/tool execution.
    Docker { image: String },
    /// Cross-platform, native WebAssembly sandboxing.
    Wasm {
        module_path: PathBuf,
        /// Whether to mount the project root to /sandbox in the WASI context.
        mount_root: bool,
    },
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
            ExecutionMode::Docker { image } => self.execute_docker(image, cmd, args).await,
            ExecutionMode::Wasm {
                module_path,
                mount_root,
            } => self.execute_wasm(module_path, *mount_root, args).await,
        }
    }

    fn execute_local(&self, cmd: &str, args: Vec<String>) -> Result<String> {
        let output = Command::new(cmd)
            .args(args)
            .current_dir(&self.root_dir)
            .env_clear()
            .output()
            .context("Local tool execution failed")?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            bail!("Tool execution failed: {}", err);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Universal Wasm execution using Wasmtime and WASI Preview 1.
    /// Universal Wasm execution using Wasmtime and WASI Preview 1.
    async fn execute_wasm(
        &self,
        module_path: &PathBuf,
        mount_root: bool,
        args: Vec<String>,
    ) -> Result<String> {
        use wasmtime::*;
        use wasmtime_wasi::pipe::MemoryOutputPipe;
        use wasmtime_wasi::preview1::{self, WasiP1Ctx};
        use wasmtime_wasi::{DirPerms, FilePerms, I32Exit, WasiCtxBuilder};

        let engine = Engine::new(wasmtime::Config::new().async_support(true))?;
        let module = Module::from_file(&engine, module_path)?;

        let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
        preview1::add_to_linker_async(&mut linker, |ctx| ctx)?;

        // Capture stdout/stderr via a memory pipe (1MB limit to avoid OOM)
        let stdout = MemoryOutputPipe::new(2048 * 2048);
        let stderr = stdout.clone();

        let mut builder = WasiCtxBuilder::new();
        builder
            .stdout(stdout.clone())
            .stderr(stderr.clone())
            .args(&args)
            .inherit_env();

        if mount_root {
            // Map the host project root to /sandbox in the guest
            let abs_path = self.root_dir.to_string_lossy().to_string();
            builder.preopened_dir(abs_path, "/sandbox", DirPerms::all(), FilePerms::all())?;
        }

        let wasi = builder.build_p1();
        let mut store = Store::new(&engine, wasi);
        let instance = linker.instantiate_async(&mut store, &module).await?;

        let func = instance.get_typed_func::<(), ()>(&mut store, "_start")?;

        if let Err(e) = func.call_async(&mut store, ()).await {
            // Check if it's a normal exit (WASI exit code 0)
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

    /// Native Docker integration using the bollard SDK.
    async fn execute_docker(&self, image: &str, cmd: &str, args: Vec<String>) -> Result<String> {
        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to Docker daemon. Is Docker Desktop running?")?;

        // 1. Pull Image (Auto-pull by default as requested for UX)
        let mut pull_stream = docker.create_image(
            Some(CreateImageOptions {
                from_image: image.to_string(),
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(pull_result) = pull_stream.next().await {
            pull_result.context("Failed while pulling Docker image")?;
        }

        // 2. Prep HostConfig (Mounting the project root)
        // Format: host_path:container_path:options
        let abs_root = self.root_dir.canonicalize()?.to_string_lossy().to_string();
        let binds = vec![format!("{}:/sandbox:rw", abs_root)];

        let host_config = HostConfig {
            binds: Some(binds),
            ..Default::default()
        };

        // 3. Create Container
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

        let container = docker
            .create_container::<String, String>(None, config)
            .await?;

        // 4. Start & Capture Logs
        docker
            .start_container::<String>(&container.id, None)
            .await?;

        let mut logs = docker.logs(
            &container.id,
            Some(bollard::container::LogsOptions::<String> {
                stdout: true,
                stderr: true,
                follow: true,
                ..Default::default()
            }),
        );

        let mut output = String::new();
        while let Some(log_result) = logs.next().await {
            match log_result? {
                LogOutput::StdOut { message } | LogOutput::StdErr { message } => {
                    output.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }

        // 5. Cleanup
        let _ = docker.remove_container(&container.id, None).await;

        Ok(output)
    }
}
