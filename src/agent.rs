use crate::{Harness, Request, executor::ToolExecutor};
use mem_core::{Context, FileStorage, ToolCall};
use mem_resilience::ResilientMemoryController;
use std::sync::Arc;
use std::path::PathBuf;
use futures_util::{StreamExt, stream::BoxStream};
use chrono::Utc;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct DeepAgentState {
    pub session_id: String,
    pub context: Arc<Context>,
    pub sandbox_root: PathBuf,
}

pub struct StepConfig {
    pub max_turns: usize,
    pub timeout_seconds: u64,
    pub fail_on_limit: bool,
}

impl Default for StepConfig {
    fn default() -> Self {
        Self {
            max_turns: 10,
            timeout_seconds: 300,
            fail_on_limit: false,
        }
    }
}

/// The DeepAgent orchestrates the Model, Harness, and Executor into a single stateful entity.
pub struct DeepAgent {
    pub harness: Harness,
    pub state: DeepAgentState,
    pub executor: Arc<dyn ToolExecutor>,
    pub memory_controller: Arc<ResilientMemoryController<FileStorage>>,
}

impl DeepAgent {
    pub fn new(
        harness: Harness, 
        state: DeepAgentState, 
        executor: Arc<dyn ToolExecutor>, 
        memory_controller: Arc<ResilientMemoryController<FileStorage>>
    ) -> Self {
        Self { harness, state, executor, memory_controller }
    }

    /// Executes a single reasoning/action step following the DeepAgent loop.
    pub async fn step(&mut self, user_input: String) -> anyhow::Result<String> {
        let mut full_content = String::new();
        let mut stream = Box::pin(self.step_stream(user_input, StepConfig::default()));
        
        while let Some(res) = stream.next().await {
            match res? {
                AgentStepEvent::TextChunk(c) => full_content.push_str(&c),
                _ => (),
            }
        }
        Ok(full_content)
    }

    /// Autonomous reasoning loop that executes tools and continues until a final answer is reached.
    pub fn step_stream(
        &mut self, 
        user_input: String,
        config: StepConfig,
    ) -> BoxStream<'_, anyhow::Result<AgentStepEvent>> {
        let stream = async_stream::try_stream! {
            // Explicitly state the chunk type to help inference
            if false { yield anyhow::Ok(AgentStepEvent::Status("".into()))?; }
            
            let start_time = std::time::Instant::now();

            // 1. Initial Prompt Handling - Add to Arc<Context>
            let mut current_context = (*self.state.context).clone();
            current_context.items.push(mem_core::MemoryItem {
                role: mem_core::MemoryRole::User,
                content: user_input.clone(),
                timestamp: Utc::now().timestamp() as u64,
                metadata: serde_json::json!({}),
            });
            self.state.context = Arc::new(current_context);

            let mut turn_count = 0;

            loop {
                turn_count += 1;
                
                // Check turn limit
                if turn_count > config.max_turns {
                    let msg = format!("Turn limit ({}) reached. Stopping.", config.max_turns);
                    yield AgentStepEvent::Status(msg.clone());
                    if config.fail_on_limit {
                        Err(anyhow::anyhow!(msg))?;
                    }
                    break;
                }

                // Check timeout
                if start_time.elapsed().as_secs() > config.timeout_seconds {
                    Err(anyhow::anyhow!("Agent step timeout after {}s", config.timeout_seconds))?;
                }

                let req = Request {
                    prompt: if turn_count == 1 { user_input.clone() } else { "Continue".to_string() },
                    context: self.state.context.clone(), // Cheap Arc clone
                    tools: vec![],
                };

                let mut stream = self.harness.run_stream(req).await?;
                let mut final_content = String::new();
                let mut tool_calls = Vec::new();
                let mut current_tool_name = String::new();
                let mut current_tool_args = String::new();

                while let Some(chunk_res) = stream.next().await {
                    let chunk = chunk_res?;
                    
                    if let Some(c) = chunk.content_delta {
                        final_content.push_str(&c);
                        yield AgentStepEvent::TextChunk(c);
                    }

                    if let Some(delta) = chunk.tool_call_delta {
                        if let Some(name) = delta.name {
                            current_tool_name.push_str(&name);
                        }
                        if let Some(args) = delta.arguments_delta {
                            current_tool_args.push_str(&args);
                        }
                    }

                    if chunk.is_final {
                        if !current_tool_name.is_empty() {
                            let arguments: serde_json::Value = serde_json::from_str(&current_tool_args)
                                .map_err(|e| {
                                    tracing::error!("Failed to parse tool arguments: {}. Raw: {}", e, current_tool_args);
                                    anyhow::anyhow!("Tool argument JSON parse error: {} for args: {}", e, current_tool_args)
                                })?;
                            tool_calls.push(ToolCall { name: current_tool_name.clone(), arguments });
                            current_tool_args.clear();
                        }
                    }
                }

                if tool_calls.is_empty() {
                    let mut current_context = (*self.state.context).clone();
                    current_context.items.push(mem_core::MemoryItem {
                        role: mem_core::MemoryRole::Assistant,
                        content: final_content,
                        timestamp: Utc::now().timestamp() as u64,
                        metadata: serde_json::json!({}),
                    });
                    self.state.context = Arc::new(current_context);
                    break;
                }

                for mut tool in tool_calls {
                    let tool_name = tool.name.clone();
                    yield AgentStepEvent::ToolStarted(tool_name.clone());
                    
                    self.harness.run_before_tool_hooks(&mut tool).await?;
                    
                    match self.executor.execute(&tool_name, tool.arguments.clone()).await {
                        Ok(mut result) => {
                            self.harness.run_after_tool_hooks(&tool, &mut result).await?;
                            yield AgentStepEvent::ToolFinished(tool_name.clone(), result.clone());
                            
                            let mut current_ctx = (*self.state.context).clone();
                            current_ctx.items.push(mem_core::MemoryItem {
                                role: mem_core::MemoryRole::Tool,
                                content: result,
                                timestamp: Utc::now().timestamp() as u64,
                                metadata: serde_json::json!({"tool": tool_name}),
                            });
                            self.state.context = Arc::new(current_ctx);
                        }
                        Err(e) => {
                            let err_msg = format!("Tool error: {}", e);
                            yield AgentStepEvent::Status(err_msg.clone());
                            
                            // Categorize error for smarter retry logic
                            let error_category = match e.to_string().to_lowercase() {
                                s if s.contains("timeout") => "transient_timeout",
                                s if s.contains("not found") => "tool_not_found",
                                s if s.contains("permission") || s.contains("denied") => "permission_denied",
                                _ => "unknown",
                            };

                            let mut current_ctx = (*self.state.context).clone();
                            current_ctx.items.push(mem_core::MemoryItem {
                                role: mem_core::MemoryRole::Tool,
                                content: err_msg,
                                timestamp: Utc::now().timestamp() as u64,
                                metadata: serde_json::json!({
                                    "tool": tool_name, 
                                    "error": true,
                                    "error_category": error_category 
                                }),
                            });
                            self.state.context = Arc::new(current_ctx);
                        }
                    }
                }
            }

            let _ = self.save_state_resilient().await;
        };
        Box::pin(stream)
    }

    /// Persists agent state atomically using temp files.
    pub async fn save_state_resilient(&self) -> anyhow::Result<()> {
        let root = PathBuf::from(".agent/sessions");
        
        // Atomic directory creation
        let _ = tokio::fs::create_dir_all(&root).await;
        
        let path = root.join(format!("session_{}.session", self.state.session_id));
        let data = serde_json::to_vec_pretty(&self.state)?;
        
        let mut optimized_context = (*self.state.context).clone();
        self.memory_controller.optimize_resilient(&mut optimized_context).await?;
        
        // Atomic write: write to temp file, then rename
        let temp_file = root.join(format!(".session_{}.tmp", self.state.session_id));
        tokio::fs::write(&temp_file, data).await?;
        tokio::fs::rename(&temp_file, path).await?;
        
        Ok(())
    }
}


pub enum AgentStepEvent {
    TextChunk(String),
    ToolStarted(String),
    ToolFinished(String, String),
    Status(String),
}
