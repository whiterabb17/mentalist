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
use crate::middleware::Middleware;
use crate::ToolCall;

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

type McpStdio = Option<(tokio::process::ChildStdin, tokio::io::BufReader<tokio::process::ChildStdout>)>;

pub struct McpExecutor {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    process: Arc<Mutex<Option<Child>>>,
    stdio: Arc<Mutex<McpStdio>>,
    call_lock: Arc<Mutex<()>>,
    id_counter: AtomicI64,
    last_error: Arc<Mutex<Option<String>>>,
    middlewares: Vec<Arc<dyn Middleware>>,
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
            last_error: Arc::new(Mutex::new(None)),
            middlewares: Vec::new(),
        }
    }

    pub fn add_middleware(&mut self, middleware: Arc<dyn Middleware>) {
        self.middlewares.push(middleware);
        self.middlewares.sort_by_key(|mw| mw.priority());
    }

    pub fn with_middleware(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.add_middleware(middleware);
        self
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

            {
                let mut stdio_guard = self.stdio.lock().await;
                *stdio_guard = Some((stdin, BufReader::new(stdout)));
            }

            *child_guard = Some(child);
            
            // Drop guards before calling initialization to avoid deadlock in raw_call_locked
            drop(child_guard);

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
                    let mut stdio_guard = self.stdio.lock().await;
                    *stdio_guard = None;
                    let mut child_guard = self.process.lock().await;
                    if let Some(mut c) = child_guard.take() {
                        let _ = c.start_kill();
                    }
                    let err_msg = e.to_string();
                    let mut err_guard = self.last_error.lock().await;
                    *err_guard = Some(err_msg.clone());
                    return Err(e);
                }
                Err(_) => {
                    let mut stdio_guard = self.stdio.lock().await;
                    *stdio_guard = None;
                    let mut child_guard = self.process.lock().await;
                    if let Some(mut c) = child_guard.take() {
                        let _ = c.start_kill();
                    }
                    let mut err_guard = self.last_error.lock().await;
                    *err_guard = Some("MCP initialization timeout".into());
                    return Err(anyhow!("MCP initialization timeout"));
                }
            }
            return Ok(()); // Success, returned early to avoid re-locking
        }
        let mut err_guard = self.last_error.lock().await;
        *err_guard = None; // Reset on success
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
        let mut skipped_lines = 0;
        const MAX_SKIPPED_LINES: usize = 100;

        // Skip non-JSON noise until we find a valid JSON line with matching ID
        let response: JsonRpcResponse = loop {
            line.clear();
            reader.read_line(&mut line).await?;
            
            if line.is_empty() {
                // Fix Issue #4: Unsafe lock ordering. 
                // We need to hold BOTH locks during cleanup to ensure atomicity.
                // Since we already hold stdio_guard, let's try to lock process.
                // To avoid deadlock, we'll try_lock or just ensure order elsewhere.
                // Standard order: process -> stdio.
                drop(stdio_guard);
                let mut pguard = self.process.lock().await;
                let mut osguard = self.stdio.lock().await;
                
                if let Some(mut child) = pguard.take() {
                    let _ = child.start_kill();
                }
                *osguard = None;
                return Err(anyhow!("MCP server closed connection unexpectedly. Process cleaned up."));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            
            // Try to parse as JSON-RPC response
            if let Ok(res) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                // Fix Issue #3: Response ID Validation
                if res.id == id {
                    break res;
                } else {
                    tracing::warn!(target: "mentalist::mcp", "[{}] Received out-of-order response: expected {}, got {}", self.command, id, res.id);
                }
            } else {
                // Not JSON-RPC, log as info and keep looking
                tracing::info!(target: "mentalist::mcp", "[{}] stdout: {}", self.command, trimmed);
                
                // Fix Issue #2: Dangerous Infinite Loop
                skipped_lines += 1;
                if skipped_lines > MAX_SKIPPED_LINES {
                    return Err(anyhow!("MCP protocol violation: exceeded max skipped lines ({}) without valid JSON-RPC response", MAX_SKIPPED_LINES));
                }
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
        let mut tool_call = ToolCall {
            name: name.to_string(),
            arguments: args,
        };

        // Fix Issue #1: Missing before_tool_call Hook
        for mw in &self.middlewares {
            mw.before_tool_call(&mut tool_call).await?;
        }

        let params = serde_json::json!({
            "name": tool_call.name,
            "arguments": tool_call.arguments
        });

        let result = self.call_rpc("tools/call", params).await?;
        
        let mut output = if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            if content.is_empty() {
                // Fix Issue #5: Silent Failure on Empty Content
                return Err(anyhow!("MCP tool '{}' returned empty content array", tool_call.name));
            }

            let mut full_text = String::new();
            for item in content {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    full_text.push_str(text);
                } else {
                    tracing::warn!(target: "mentalist::mcp", "[{}] Tool '{}' returned content item without text field: {:?}", self.command, tool_call.name, item);
                }
            }
            
            if full_text.is_empty() && !content.is_empty() {
                return Err(anyhow!("MCP tool '{}' returned content but no text fields were found", tool_call.name));
            }
            
            full_text
        } else {
            serde_json::to_string(&result)?
        };

        // Run after_tool_call hooks
        for mw in &self.middlewares {
            mw.after_tool_call(&tool_call, &mut output).await?;
        }

        Ok(output)
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

    fn status(&self) -> String {
        if let Ok(guard) = self.process.try_lock() {
            if guard.is_some() {
                "Connected".to_string()
            } else if let Ok(err_guard) = self.last_error.try_lock() {
                if let Some(err) = &*err_guard {
                    format!("Error: {}", err)
                } else {
                    "Disconnected".to_string()
                }
            } else {
                "Disconnected".to_string()
            }
        } else {
            "Starting...".to_string()
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
