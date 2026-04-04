use crate::executor::ToolExecutor;
use mem_core::ToolDefinition;
use async_trait::async_trait;
use anyhow::{Result, Context, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::{Command, Child};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Value,
    id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<JsonRpcError>,
    id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

pub struct McpExecutor {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    process: Arc<Mutex<Option<Child>>>,
    stdio: Arc<Mutex<Option<(tokio::process::ChildStdin, tokio::io::BufReader<tokio::process::ChildStdout>)>>>,
    call_lock: Arc<Mutex<()>>,
    id_counter: AtomicI64,
}

impl Drop for McpExecutor {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.process.try_lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.start_kill();
            }
        }
    }
}

impl McpExecutor {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            env: HashMap::new(),
            process: Arc::new(Mutex::new(None)),
            stdio: Arc::new(Mutex::new(None)),
            call_lock: Arc::new(Mutex::new(())),
            id_counter: AtomicI64::new(1),
        }
    }

    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    async fn ensure_process_locked(&self) -> Result<()> {
        let mut child_guard = self.process.lock().await;
        if child_guard.is_none() {
            tracing::info!("Starting MCP server: {} {:?}", self.command, self.args);
            
            let mut cmd = Command::new(&self.command);
            cmd.args(&self.args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .envs(&self.env);
            
            let mut child = cmd.spawn()
                .context(format!("Failed to start MCP server: {}", self.command))?;
            
            if let Some(stderr) = child.stderr.take() {
                let cmd_name = self.command.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        tracing::warn!(target: "mentalist::mcp", "[{}] stderr: {}", cmd_name, line);
                    }
                });
            }

            let stdin = child.stdin.take().context("Failed to open stdin")?;
            let stdout = child.stdout.take().context("Failed to open stdout")?;

            let mut stdio_guard = self.stdio.lock().await;
            *stdio_guard = Some((stdin, BufReader::new(stdout)));

            *child_guard = Some(child);
            
            let init_future = async {
                let params = serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "Mentalist",
                        "version": "0.3.0"
                    }
                });
                self.raw_call_locked("initialize", params).await
            };

            let res = tokio::time::timeout(std::time::Duration::from_secs(10), init_future).await;
            match res {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    *stdio_guard = None;
                    if let Some(mut c) = child_guard.take() {
                        let _ = c.start_kill();
                    }
                    return Err(e);
                }
                Err(_) => {
                    *stdio_guard = None;
                    if let Some(mut c) = child_guard.take() {
                        let _ = c.start_kill();
                    }
                    return Err(anyhow!("MCP initialization timeout"));
                }
            }
        }
        Ok(())
    }

    async fn call_rpc(&self, method: &str, params: Value) -> Result<Value> {
        let _guard = self.call_lock.lock().await;
        self.ensure_process_locked().await?;
        self.raw_call_locked(method, params).await
    }

    async fn raw_call_locked(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.id_counter.fetch_add(1, Ordering::SeqCst);
        
        // Use persistent read/write buffers
        let mut stdio_guard = self.stdio.lock().await;
        let (stdin, reader) = stdio_guard.as_mut().context("MCP Process stdio not initialized")?;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id,
        };

        let req_text = serde_json::to_string(&request)? + "\n";
        stdin.write_all(req_text.as_bytes()).await?;
        stdin.flush().await?;

        let mut line = String::new();
        // Skip non-JSON noise (like terminal welcome messages) until we find a valid JSON line
        let response: JsonRpcResponse = loop {
            line.clear();
            reader.read_line(&mut line).await?;
            
            if line.is_empty() {
                drop(stdio_guard);
                let mut pguard = self.process.lock().await;
                if let Some(mut child) = pguard.take() {
                    let _ = child.start_kill();
                }
                let mut osguard = self.stdio.lock().await;
                *osguard = None;
                return Err(anyhow!("MCP server closed connection unexpectedly. Process cleaned up."));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            
            // Try to parse as JSON-RPC response
            if let Ok(res) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                break res;
            } else {
                // Not JSON-RPC, log as info and keep looking
                tracing::info!(target: "mentalist::mcp", "[{}] stdout: {}", self.command, trimmed);
            }
        };

        // Fix Issue #4: Dangerous Unwrap masking error body
        match (response.result, response.error) {
            (Some(result), None) => Ok(result),
            (None, Some(err)) => Err(anyhow!("MCP RPC Error ({}): {}", err.code, err.message)),
            (None, None) => Err(anyhow!("MCP protocol violation: no result or error")),
            (Some(_), Some(_)) => Err(anyhow!("MCP protocol violation: both result and error")),
        }
    }
}

#[async_trait]
impl ToolExecutor for McpExecutor {
    async fn execute(&self, name: &str, args: Value) -> Result<String> {
        let params = serde_json::json!({
            "name": name,
            "arguments": args
        });

        let result = self.call_rpc("tools/call", params).await?;
        
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            let mut full_text = String::new();
            for item in content {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    full_text.push_str(text);
                }
            }
            Ok(full_text)
        } else {
            Ok(serde_json::to_string(&result)?)
        }
    }

    async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let result = self.call_rpc("tools/list", Value::Object(serde_json::Map::new())).await?;
        
        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            let mut definitions = Vec::new();
            for tool in tools {
                let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string();
                let description = tool.get("description").and_then(|d| d.as_str()).unwrap_or_default().to_string();
                let parameters = tool.get("inputSchema").cloned().unwrap_or(serde_json::json!({}));
                
                definitions.push(ToolDefinition {
                    name,
                    description,
                    parameters,
                });
            }
            Ok(definitions)
        } else {
            Ok(vec![])
        }
    }
}

/// Helper for launching common MCP servers using npx
pub struct BuiltinMcp;

impl BuiltinMcp {
    pub fn filesystem(paths: Vec<String>) -> McpExecutor {
        let mut args = vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string()];
        args.extend(paths);
        let cmd = if cfg!(target_os = "windows") { "npx.cmd" } else { "npx" };
        McpExecutor::new(cmd.to_string(), args)
    }

    pub fn firecrawl(api_key: String) -> McpExecutor {
        let args = vec!["-y".to_string(), "firecrawl-mcp".to_string()];
        let mut env = HashMap::new();
        env.insert("FIRECRAWL_API_KEY".to_string(), api_key);
        let cmd = if cfg!(target_os = "windows") { "npx.cmd" } else { "npx" };
        McpExecutor::new(cmd.to_string(), args).with_env(env)
    }
}
