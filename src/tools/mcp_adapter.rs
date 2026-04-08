use async_trait::async_trait;
use crate::tools::{Tool, ToolSchema};
use serde_json::Value;
use std::sync::Arc;
use anyhow::Result;
use std::collections::HashMap;

use tokio::process::{Command, Child};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::Mutex;

pub struct McpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<std::path::PathBuf>,
    pub initialization_timeout: std::time::Duration,
    process: Arc<Mutex<Option<Child>>>,
    stdio: Arc<Mutex<Option<(tokio::process::ChildStdin, BufReader<tokio::process::ChildStdout>)>>>,
    id_counter: AtomicI64,
}

impl McpServer {
    pub fn new(name: String, command: String, args: Vec<String>) -> Self {
        Self {
            name,
            command,
            args,
            env: HashMap::new(),
            cwd: None,
            initialization_timeout: std::time::Duration::from_secs(30),
            process: Arc::new(Mutex::new(None)),
            stdio: Arc::new(Mutex::new(None)),
            id_counter: AtomicI64::new(1),
        }
    }

    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    pub fn with_cwd(mut self, cwd: std::path::PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.initialization_timeout = timeout;
        self
    }

    async fn ensure_started(&self) -> Result<()> {
        let mut process_guard = self.process.lock().await;
        if process_guard.is_none() {
            let mut cmd = Command::new(&self.command);
            cmd.args(&self.args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .envs(&self.env);
            
            if let Some(ref cwd) = self.cwd {
                cmd.current_dir(cwd);
            }

            let mut child = cmd.spawn()?;
            
            if let Some(stderr) = child.stderr.take() {
                let name = self.name.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        tracing::warn!(target: "mentalist::mcp", "[{}] stderr: {}", name, line);
                    }
                });
            }

            let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("Failed to open stdin"))?;
            let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("Failed to open stdout"))?;

            {
                let mut stdio_guard = self.stdio.lock().await;
                *stdio_guard = Some((stdin, BufReader::new(stdout)));
            }
            *process_guard = Some(child);
            
            // Initialization (initialize request)
            drop(process_guard); // Release process lock before network/IO call
            
            if let Err(e) = self.initialize_protocol().await {
                tracing::error!(target: "mentalist::mcp", "[{}] MCP initialization failed: {}", self.name, e);
                // Cleanup on failure
                self.stop().await.ok();
                return Err(e);
            }
        }
        Ok(())
    }

    async fn initialize_protocol(&self) -> Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "sampling": {}
            },
            "clientInfo": {
                "name": "mentalist",
                "version": "0.3.5"
            }
        });
        self.raw_call("initialize", params).await?;
        self.raw_notification("notifications/initialized", serde_json::json!({})).await?;
        Ok(())
    }

    async fn raw_call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.id_counter.fetch_add(1, Ordering::SeqCst);
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let mut stdio_guard = self.stdio.lock().await;
        let (stdin, stdout) = stdio_guard.as_mut().ok_or_else(|| anyhow::anyhow!("MCP process not started"))?;
        
        let req_str = format!("{}\n", serde_json::to_string(&request)?);
        stdin.write_all(req_str.as_bytes()).await?;
        stdin.flush().await?;

        let mut line = String::new();
        let timeout = self.initialization_timeout;
        
        tokio::select! {
            res = stdout.read_line(&mut line) => {
                res?;
            }
            _ = tokio::time::sleep(timeout) => {
                anyhow::bail!("MCP raw_call timeout ({}s) for method: {}", timeout.as_secs(), method);
            }
        }
        
        let response: Value = serde_json::from_str(&line)?;
        if let Some(err) = response.get("error") {
            anyhow::bail!("MCP error: {}", err);
        }
        
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn raw_notification(&self, method: &str, params: Value) -> Result<()> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        let mut stdio_guard = self.stdio.lock().await;
        let (stdin, _) = stdio_guard.as_mut().ok_or_else(|| anyhow::anyhow!("MCP process not started"))?;
        
        let req_str = format!("{}\n", serde_json::to_string(&request)?);
        stdin.write_all(req_str.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    pub async fn call(&self, name: &str, arguments: Value) -> Result<Value> {
        self.ensure_started().await?;
        self.raw_call("tools/call", serde_json::json!({
            "name": name,
            "arguments": arguments
        })).await
    }

    pub async fn list_tools(&self) -> Result<Vec<(String, String, Value)>> {
        self.ensure_started().await?;
        let res = self.raw_call("tools/list", serde_json::json!({})).await?;
        let mut tools = Vec::new();
        if let Some(tools_arr) = res.get("tools").and_then(|t| t.as_array()) {
            for t in tools_arr {
                let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                let desc = t.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
                let params = t.get("inputSchema").cloned().unwrap_or(serde_json::json!({"type": "object"}));
                tools.push((name, desc, params));
            }
        }
        Ok(tools)
    }

    pub async fn stop(&self) -> Result<()> {
        let mut process_guard = self.process.lock().await;
        if let Some(mut child) = process_guard.take() {
            tracing::info!(target: "mentalist::mcp", "[{}] Stopping MCP server process", self.name);
            let _ = child.kill().await;
        }
        let mut stdio_guard = self.stdio.lock().await;
        *stdio_guard = None;
        Ok(())
    }
}

pub struct McpTool {
    pub server: Arc<McpServer>,
    pub source: String,
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[async_trait]
impl Tool for McpTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
            source: self.source.clone(),
        }
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        self.server.call(&self.name, input).await
    }

    fn source(&self) -> String {
        self.source.clone()
    }
}

pub struct BuiltinMcp;

impl BuiltinMcp {
    pub fn filesystem(paths: Vec<String>, root: Option<&std::path::Path>) -> Result<McpServer> {
        let (cmd, args) = if let Some(r) = root {
            let bin_name = if cfg!(target_os = "windows") { "mcp-server-filesystem.cmd" } else { "mcp-server-filesystem" };
            let bin_path = r.join("filesystem").join("node_modules").join(".bin").join(bin_name);
            
            if bin_path.exists() {
                (bin_path.to_string_lossy().to_string(), paths)
            } else {
                let npx = if cfg!(target_os = "windows") { "npx.cmd" } else { "npx" };
                let mut npx_args = vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string()];
                npx_args.extend(paths);
                (npx.to_string(), npx_args)
            }
        } else {
            let npx = if cfg!(target_os = "windows") { "npx.cmd" } else { "npx" };
            let mut npx_args = vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string()];
            npx_args.extend(paths);
            (npx.to_string(), npx_args)
        };

        let mut server = McpServer::new("filesystem".to_string(), cmd, args);
        server = server.with_cwd(std::env::current_dir()?);
        Ok(server)
    }

    pub fn duckduckgo(root: Option<&std::path::Path>) -> Result<McpServer> {
        let cmd = if cfg!(target_os = "windows") { "npx.cmd" } else { "npx" };
        let args = vec!["-y".to_string(), "duckduckgo-mcp-server".to_string()];
        let mut server = McpServer::new("duckduckgo".to_string(), cmd.to_string(), args);
        if let Some(p) = root {
            let server_dir = p.join("duckduckgo");
            server = server.with_cwd(server_dir);
        } else {
            server = server.with_cwd(std::env::current_dir()?);
        }
        Ok(server)
    }

    pub fn firecrawl(api_key: String, root: Option<&std::path::Path>) -> Result<McpServer> {
        let cmd = if cfg!(target_os = "windows") { "npx.cmd" } else { "npx" };
        let args = vec!["-y".to_string(), "firecrawl-mcp".to_string()];
        let mut env = HashMap::new();
        env.insert("FIRECRAWL_API_KEY".to_string(), api_key);
        let mut server = McpServer::new("firecrawl".to_string(), cmd.to_string(), args).with_env(env);
        if let Some(p) = root {
            let server_dir = p.join("firecrawl");
            server = server.with_cwd(server_dir);
        } else {
            server = server.with_cwd(std::env::current_dir()?);
        }
        Ok(server)
    }

    /// Installs a package into its own isolated subdirectory under `root`.
    /// The directory name is derived consistently with the factory methods:
    ///   `@modelcontextprotocol/server-filesystem` → `<root>/filesystem/`
    ///   `duckduckgo-mcp-server`                   → `<root>/duckduckgo/`
    ///   `firecrawl-mcp`                           → `<root>/firecrawl/`
    pub async fn ensure_mcp_installed(root: &std::path::Path, package: &str) -> Result<()> {
        let dir_name = match package {
            "@modelcontextprotocol/server-filesystem" => "filesystem",
            "duckduckgo-mcp-server" => "duckduckgo",
            "firecrawl-mcp" => "firecrawl",
            other => other,
        };
        let server_dir = root.join(dir_name);
        if !server_dir.exists() {
            std::fs::create_dir_all(&server_dir)?;
        }
        let node_modules = server_dir.join("node_modules");
        // Skip if already installed
        if node_modules.exists()
            && std::fs::read_dir(&node_modules)
                .map(|mut d| d.next().is_some())
                .unwrap_or(false)
        {
            return Ok(());
        }
        tracing::info!("Installing MCP package '{}' into {:?}", package, server_dir);
        let npm_cmd = if cfg!(target_os = "windows") { "npm.cmd" } else { "npm" };
        let status = Command::new(npm_cmd)
            .arg("install")
            .arg("--no-package-lock")
            .arg("--no-save")
            .arg(package)
            .current_dir(&server_dir)
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("Failed to install MCP package '{}' into {:?}", package, server_dir);
        }
        Ok(())
    }
}
