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
        if chunks.is_empty() || !chunks.last().unwrap().as_ref().map_or(false, |c| c.is_final) {
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
    let provider = Box::new(MockToolModel { tool_name: "test_tool".into() });
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

    let mut agent = DeepAgent::new(harness, state, executor, memory_controller);
    let _ = agent.step("run".into()).await.unwrap();
    
    // Verify categorization
    let tool_item = agent.state.context.items.iter().find(|i| i.role == MemoryRole::Tool).unwrap();
    assert_eq!(tool_item.metadata["error_category"], "transient_timeout");
}

#[tokio::test]
async fn test_agent_error_categorization_not_found() {
    let provider = Box::new(MockToolModel { tool_name: "bad_tool".into() });
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

    let mut agent = DeepAgent::new(harness, state, executor, memory_controller);
    let _ = agent.step("run".into()).await.unwrap();
    
    // Verify categorization
    let tool_item = agent.state.context.items.iter().find(|i| i.role == MemoryRole::Tool).unwrap();
    assert_eq!(tool_item.metadata["error_category"], "tool_not_found");
}
