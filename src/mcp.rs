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
    process: Arc<Mutex<Option<Child>>>,
}

impl McpExecutor {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            process: Arc::new(Mutex::new(None)),
        }
    }

    async fn ensure_process(&self) -> Result<()> {
        let mut child_guard = self.process.lock().await;
        if child_guard.is_none() {
            tracing::info!("Starting MCP server: {} {:?}", self.command, self.args);
            let child = Command::new(&self.command)
                .args(&self.args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .context(format!("Failed to start MCP server: {}", self.command))?;
            *child_guard = Some(child);
        }
        Ok(())
    }

    async fn call_rpc(&self, method: &str, params: Value) -> Result<Value> {
        self.ensure_process().await?;
        
        let mut child_guard = self.process.lock().await;
        let child = child_guard.as_mut().unwrap();
        
        let stdin = child.stdin.as_mut().context("Failed to open stdin")?;
        let stdout = child.stdout.as_mut().context("Failed to open stdout")?;
        let mut reader = BufReader::new(stdout);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: 1, // Simple sequential ID could be improved
        };

        let req_text = serde_json::to_string(&request)? + "\n";
        stdin.write_all(req_text.as_bytes()).await?;
        stdin.flush().await?;

        let mut line = String::new();
        reader.read_line(&mut line).await?;
        
        if line.is_empty() {
            // Process might have died
            *child_guard = None;
            return Err(anyhow!("MCP server closed connection"));
        }

        let response: JsonRpcResponse = serde_json::from_str(&line)
            .context(format!("Failed to parse MCP response: {}", line))?;

        if let Some(err) = response.error {
            return Err(anyhow!("MCP RPC Error ({}): {}", err.code, err.message));
        }

        Ok(response.result.unwrap_or(Value::Null))
    }
}

#[async_trait]
impl ToolExecutor for McpExecutor {
    async fn execute(&self, name: &str, args: Value) -> Result<String> {
        // MCP tools/call expects { name: "...", arguments: { ... } }
        let params = serde_json::json!({
            "name": name,
            "arguments": args
        });

        let result = self.call_rpc("tools/call", params).await?;
        
        // MCP response for tools/call usually contains a 'content' array
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
