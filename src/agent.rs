use crate::{Harness, Request, executor::SandboxedExecutor};
use mem_core::{Context, FileStorage, ToolCall};
use mem_resilience::ResilientMemoryController;
use std::sync::Arc;
use std::path::PathBuf;
use futures_util::StreamExt;
use async_stream::try_stream;
use chrono::Utc;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct DeepAgentState {
    pub session_id: String,
    pub context: Context,
    pub sandbox_root: PathBuf,
}

/// The DeepAgent orchestrates the Model, Harness, and Executor into a single stateful entity.
pub struct DeepAgent {
    pub harness: Harness,
    pub state: DeepAgentState,
    pub executor: SandboxedExecutor,
    pub memory_controller: Arc<ResilientMemoryController<FileStorage>>,
}

impl DeepAgent {
    pub fn new(
        harness: Harness, 
        state: DeepAgentState, 
        executor: SandboxedExecutor, 
        memory_controller: Arc<ResilientMemoryController<FileStorage>>
    ) -> Self {
        Self { harness, state, executor, memory_controller }
    }

    /// Executes a single reasoning/action step following the DeepAgent loop.
    pub async fn step(&mut self, user_input: String) -> anyhow::Result<String> {
        let mut full_content = String::new();
        let mut stream = Box::pin(self.step_stream(user_input));
        
        while let Some(res) = stream.next().await {
            match res? {
                AgentStepEvent::TextChunk(c) => full_content.push_str(&c),
                _ => (),
            }
        }
        Ok(full_content)
    }

    /// Autonomous reasoning loop that executes tools and continues until a final answer is reached.
    pub fn step_stream(&mut self, user_input: String) -> impl futures_util::Stream<Item = anyhow::Result<AgentStepEvent>> + '_ {
        try_stream! {
            // 1. Initial Prompt Handling
            self.state.context.items.push(mem_core::MemoryItem {
                role: mem_core::MemoryRole::User,
                content: user_input.clone(),
                timestamp: Utc::now().timestamp() as u64,
                metadata: serde_json::json!({}),
            });

            let mut turn_count = 0;
            const MAX_TURNS: usize = 10;

            loop {
                turn_count += 1;
                if turn_count > MAX_TURNS {
                    yield AgentStepEvent::Status("Turn limit reached. Stopping.".to_string());
                    break;
                }

                let req = Request {
                    prompt: if turn_count == 1 { user_input.clone() } else { "Continue".to_string() },
                    context: self.state.context.clone(),
                };

                let mut stream = self.harness.run_stream(req).await?;
                let mut final_content = String::new();
                let mut tool_calls = Vec::new();
                let mut current_tool_name = String::new();
                let mut current_tool_args = String::new();

                while let Some(chunk_res) = stream.next().await {
                    let chunk = chunk_res?;
                    
                    // Accumulate text
                    if let Some(c) = chunk.content_delta {
                        final_content.push_str(&c);
                        yield AgentStepEvent::TextChunk(c);
                    }

                    // Reconstruct tool calls from deltas
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
                            let arguments: serde_json::Value = serde_json::from_str(&current_tool_args).unwrap_or(serde_json::json!({}));
                            tool_calls.push(ToolCall { name: current_tool_name.clone(), arguments });
                            current_tool_name.clear();
                            current_tool_args.clear();
                        }
                    }
                }

                // If no tools, finalize and break
                if tool_calls.is_empty() {
                    self.state.context.items.push(mem_core::MemoryItem {
                        role: mem_core::MemoryRole::Assistant,
                        content: final_content,
                        timestamp: Utc::now().timestamp() as u64,
                        metadata: serde_json::json!({}),
                    });
                    break;
                }

                // Execute tools sequentially
                for mut tool in tool_calls {
                    yield AgentStepEvent::ToolStarted(tool.name.clone());
                    
                    self.harness.run_before_tool_hooks(&mut tool).await?;
                    
                    let args_vec: Vec<String> = if let Some(obj) = tool.arguments.as_object() {
                        obj.values().map(|v| v.as_str().unwrap_or_default().to_string()).collect()
                    } else {
                        vec![]
                    };

                    match self.executor.execute(&tool.name, args_vec).await {
                        Ok(mut result) => {
                            self.harness.run_after_tool_hooks(&tool, &mut result).await?;
                            yield AgentStepEvent::ToolFinished(tool.name.clone(), result.clone());
                            
                            self.state.context.items.push(mem_core::MemoryItem {
                                role: mem_core::MemoryRole::Tool,
                                content: result,
                                timestamp: Utc::now().timestamp() as u64,
                                metadata: serde_json::json!({"tool": tool.name}),
                            });
                        }
                        Err(e) => {
                            let err_msg = format!("Tool error: {}", e);
                            yield AgentStepEvent::Status(err_msg.clone());
                            self.state.context.items.push(mem_core::MemoryItem {
                                role: mem_core::MemoryRole::Tool,
                                content: err_msg,
                                timestamp: Utc::now().timestamp() as u64,
                                metadata: serde_json::json!({"tool": tool.name, "error": true}),
                            });
                        }
                    }
                }
            }

            self.save_state_resilient().await?;
        }
    }

    /// Persists agent state using the ResilientMemoryController (Gap 9)
    pub async fn save_state_resilient(&self) -> anyhow::Result<()> {
        let root = PathBuf::from(".agent/sessions");
        if !root.exists() {
            std::fs::create_dir_all(&root)?;
        }
        
        let path = root.join(format!("session_{}.session", self.state.session_id));
        let data = serde_json::to_vec_pretty(&self.state)?;
        
        self.memory_controller.optimize_resilient(&mut self.state.context.clone()).await?;
        
        std::fs::write(path, data)?;
        Ok(())
    }
}

pub enum AgentStepEvent {
    TextChunk(String),
    ToolStarted(String),
    ToolFinished(String, String),
    Status(String),
}
