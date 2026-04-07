use mentalist::{Harness, DeepAgent, DeepAgentState, Request, Response, ToolCall, ModelProvider, ResponseChunk, Context};
use mentalist::executor::ToolExecutor;
use mem_core::{MemoryRole, FileStorage};
use mem_resilience::ResilientMemoryController;
use async_trait::async_trait;
use std::sync::Arc;
use std::path::PathBuf;
use futures_util::stream::{self, BoxStream};

struct MockToolModel {
    pub tool_name: String,
}

#[async_trait]
impl mem_core::LlmClient for MockToolModel {
    async fn completion(&self, _prompt: &str) -> anyhow::Result<String> {
        Ok("Mocked Completion".to_string())
    }
}

#[async_trait]
impl ModelProvider for MockToolModel {
    async fn complete(&self, req: Request) -> anyhow::Result<Response> {
        if req.context.items.iter().any(|i| i.role == MemoryRole::Tool) {
            Ok(Response { content: "Done".to_string(), tool_calls: vec![] })
        } else {
            Ok(Response { 
                content: "I need to use a tool".to_string(), 
                tool_calls: vec![ToolCall {
                    name: self.tool_name.clone(),
                    arguments: serde_json::json!({}),
                }]
            })
        }
    }

    async fn stream_complete(&self, req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        let res = self.complete(req).await?;
        let mut chunks = vec![];
        if !res.content.is_empty() {
            chunks.push(Ok(ResponseChunk {
                content_delta: Some(res.content),
                tool_call_delta: None,
                usage: None,
                is_final: false,
            }));
        }
        for tool in res.tool_calls {
            chunks.push(Ok(ResponseChunk {
                content_delta: None,
                tool_call_delta: Some(mentalist::ToolCallDelta {
                    name: Some(tool.name),
                    arguments_delta: Some(tool.arguments.to_string()),
                }),
                usage: None,
                is_final: true,
            }));
        }
        if chunks.is_empty() || !chunks.last().unwrap().as_ref().is_ok_and(|c| c.is_final) {
            chunks.push(Ok(ResponseChunk {
               content_delta: None,
               tool_call_delta: None,
               usage: None,
               is_final: true,
           }));
       }
        Ok(Box::pin(stream::iter(chunks)))
    }
}

struct MockErrorExecutor {
    pub error_msg: String,
}

#[async_trait]
impl ToolExecutor for MockErrorExecutor {
    async fn execute(&self, _name: &str, _args: serde_json::Value) -> anyhow::Result<String> {
        anyhow::bail!(self.error_msg.clone())
    }
    async fn list_tools(&self) -> anyhow::Result<Vec<mem_core::ToolDefinition>> { Ok(vec![]) }
}

#[tokio::test]
async fn test_agent_error_categorization_timeout() {
    let provider = Arc::new(MockToolModel { tool_name: "test_tool".into() });
    let harness = Harness::new(provider);
    let executor = Arc::new(MockErrorExecutor { error_msg: "Request timeout".into() });
    
    let storage = FileStorage::new(PathBuf::from("/tmp/mentalist_test"));
    let memory_controller = Arc::new(ResilientMemoryController::new(
        Arc::new(brain::Brain::new(mem_core::MindPalaceConfig::default(), None, None)),
        storage,
        3
    ));

    let state = DeepAgentState {
        session_id: "test".into(),
        context: Arc::new(Context { items: vec![] }),
        sandbox_root: PathBuf::from("."),
    };

    let mut agent = DeepAgent::new(harness, state, executor, memory_controller, None);
    let _ = agent.step("run".into()).await.unwrap();
    
    // Verify categorization
    let tool_item = agent.state.context.items.iter().find(|i| i.role == MemoryRole::Tool).unwrap();
    assert_eq!(tool_item.metadata["error_category"], "transient_timeout");
}

#[tokio::test]
async fn test_agent_error_categorization_not_found() {
    let provider = Arc::new(MockToolModel { tool_name: "bad_tool".into() });
    let harness = Harness::new(provider);
    let executor = Arc::new(MockErrorExecutor { error_msg: "Tool not found in path".into() });
    
    let storage = FileStorage::new(PathBuf::from("/tmp/mentalist_test2"));
    let memory_controller = Arc::new(ResilientMemoryController::new(
        Arc::new(brain::Brain::new(mem_core::MindPalaceConfig::default(), None, None)),
        storage,
        3
    ));

    let state = DeepAgentState {
        session_id: "test2".into(),
        context: Arc::new(Context { items: vec![] }),
        sandbox_root: PathBuf::from("."),
    };

    let mut agent = DeepAgent::new(harness, state, executor, memory_controller, None);
    let _ = agent.step("run".into()).await.unwrap();
    
    // Verify categorization
    let tool_item = agent.state.context.items.iter().find(|i| i.role == MemoryRole::Tool).unwrap();
    assert_eq!(tool_item.metadata["error_category"], "tool_not_found");
}

#[tokio::test]
async fn test_timeout_protection() {
    let provider = Arc::new(MockToolModel { tool_name: "test_tool".into() });
    let harness = Harness::new(provider);
    let executor = Arc::new(MockErrorExecutor { error_msg: "Connection timeout while calling tool".into() });
    
    let storage = FileStorage::new(PathBuf::from("/tmp/mentalist_test3"));
    let brain = Arc::new(brain::Brain::new(mem_core::MindPalaceConfig::default(), None, None));
    let memory_controller = Arc::new(ResilientMemoryController::new(brain, storage, 3));

    let state = DeepAgentState {
        session_id: "test3".into(),
        context: Arc::new(Context { items: vec![] }),
        sandbox_root: PathBuf::from("."),
    };

    let mut agent = DeepAgent::new(harness, state, executor, memory_controller, None);
    let _ = agent.step("run".into()).await.unwrap();
    
    // Verify categorization
    let tool_item = agent.state.context.items.iter().find(|i| i.role == MemoryRole::Tool).unwrap();
    assert_eq!(tool_item.metadata["error_category"], "transient_timeout");
}

#[tokio::test]
async fn test_command_whitelist_enforcement() {
    use mentalist::executor::{SandboxedExecutor, ExecutionMode};
    
    let temp_dir = std::env::temp_dir().join("mentalist_whitelist_test");
    std::fs::create_dir_all(&temp_dir).unwrap();
    
    let executor = SandboxedExecutor::new(
        ExecutionMode::Local,
        temp_dir.clone(),
        None
    ).unwrap();
    
    // "rm" is not in the default whitelist
    let result = executor.execute("rm", serde_json::json!({ "path": "/etc/passwd" })).await;
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not in the whitelist"));
    
    std::fs::remove_dir_all(temp_dir).unwrap();
}

struct ErrorMiddleware;
#[async_trait]
impl mentalist::middleware::Middleware for ErrorMiddleware {
    fn name(&self) -> &str { "ErrorMiddleware" }
    async fn before_ai_call(&self, _req: &mut Request) -> anyhow::Result<()> {
        anyhow::bail!("Middleware intentional failure")
    }
}

#[tokio::test]
async fn test_middleware_error_propagation() {
    let provider = Arc::new(MockToolModel { tool_name: "test_tool".into() });
    let mut harness = Harness::new(provider);
    harness.add_middleware(Arc::new(ErrorMiddleware));
    
    let executor = Arc::new(MockErrorExecutor { error_msg: "ok".into() });
    
    let storage = FileStorage::new(PathBuf::from("/tmp/mentalist_test4"));
    let brain = Arc::new(brain::Brain::new(mem_core::MindPalaceConfig::default(), None, None));
    let memory_controller = Arc::new(ResilientMemoryController::new(brain, storage, 3));

    let state = DeepAgentState {
        session_id: "test4".into(),
        context: Arc::new(Context { items: vec![] }),
        sandbox_root: PathBuf::from("."),
    };

    let mut agent = DeepAgent::new(harness, state, executor, memory_controller, None);
    let result = agent.step("run".into()).await;
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Middleware 'ErrorMiddleware' failure"));
}

#[tokio::test]
async fn test_wasm_execution_smoke() {
    use mentalist::executor::{SandboxedExecutor, ExecutionMode};
    use std::collections::HashMap;
    
    let temp_dir = std::env::temp_dir().join("mentalist_wasm_test");
    std::fs::create_dir_all(&temp_dir).unwrap();
    
    let wasm_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("wasm_tools")
        .join("wasm_tools.wasm");
        
    if !wasm_path.exists() {
        // Wasm binary not built — skip gracefully rather than fail.
        // Build with `cargo build -p wasm_tools` to enable this test.
        eprintln!("SKIP test_wasm_execution_smoke: wasm binary not found at {:?}", wasm_path);
        std::fs::remove_dir_all(&temp_dir).ok();
        return;
    }
    
    let mut executor = SandboxedExecutor::new(
        ExecutionMode::Wasm {
            module_path: Some(wasm_path),
            mount_root: true,
            env_vars: HashMap::new(),
        },
        temp_dir.clone(),
        None
    ).unwrap();
    executor.validator.allowed_cmds.push("stats".to_string());
    
    let result = executor.execute("stats", serde_json::json!({ "text": "hello world" })).await;
    
    match result {
        // The wasm-tools feature may be compiled out in CI — skip gracefully.
        Err(ref e) if e.to_string().contains("Wasm tools feature disabled") => {
            eprintln!("SKIP test_wasm_execution_smoke: wasm-tools feature not enabled");
        }
        Err(e) => panic!("Wasm execution failed unexpectedly: {}", e),
        Ok(output) => {
            assert!(output.contains("Chars: 11"), "Expected char count in output");
            assert!(output.contains("Words: 2"), "Expected word count in output");
        }
    }
    
    std::fs::remove_dir_all(temp_dir).ok();
}
