use crate::{Harness, Request, executor::ToolExecutor, config::AgentConfig, error::Result as MentalistResult, error::MentalistError};
use mem_core::{Context, FileStorage, ToolCall};
use mem_resilience::ResilientMemoryController;
use mem_dreamer::DreamScheduler;
use std::sync::Arc;
use std::path::PathBuf;
use futures_util::{StreamExt, stream::BoxStream};
use chrono::Utc;

/// Persistent state for a DeepAgent session.
///
/// Contains the session ID, conversation context, and the sandbox root directory.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct DeepAgentState {
    pub session_id: String,
    /// Shared pointer to the conversation history/context.
    pub context: Arc<Context>,
    /// Root directory for local file operations and tool execution.
    pub sandbox_root: PathBuf,
}

// Removed redundant StepConfig as it is now part of crate::config::AgentConfig

/// The DeepAgent orchestrates the Model, Harness, and Executor into a single stateful entity.
///
/// It implements the "Deep Reasoning" loop where the model can call tools, process their results,
/// and continue reasoning before providing a final response to the user.
///
/// # Example
/// ```no_run
/// use mentalist::{DeepAgent, DeepAgentState, Harness, provider::OpenAiProvider};
/// use mentalist::executor::{SandboxedExecutor, ExecutionMode};
/// use std::sync::Arc;
/// use std::path::PathBuf;
///
/// # async fn example() -> anyhow::Result<()> {
/// let provider = Arc::new(OpenAiProvider::new("gpt-4o".into(), "key".into()));
/// let harness = Harness::new(provider);
/// let state = DeepAgentState {
///     session_id: "example".into(),
///     context: Arc::new(Default::default()),
///     sandbox_root: PathBuf::from("./sandbox"),
/// };
/// let executor = Arc::new(SandboxedExecutor::new(ExecutionMode::Local, PathBuf::from("./"), None)?);
/// // For doctest purposes, we assume a correctly initialized controller.
/// let brain = Arc::new(brain::Brain::new(mem_core::MindPalaceConfig::default(), None, None));
/// let storage = mem_core::FileStorage::new(std::path::PathBuf::from("."));
/// let memory = Arc::new(mem_resilience::ResilientMemoryController::new(brain, storage, 3));
///
/// let mut agent = DeepAgent::new(harness, state, executor, memory, None);
/// let response = agent.step("Calculate 123 * 456".into()).await?;
/// println!("Agent says: {}", response);
/// # Ok(())
/// # }
/// ```
pub struct DeepAgent {
    pub harness: Harness,
    pub state: DeepAgentState,
    pub executor: Arc<dyn ToolExecutor>,
    pub memory_controller: Arc<ResilientMemoryController<FileStorage>>,
    pub scheduler: Option<DreamScheduler<FileStorage>>,
    /// Prevents concurrent state file writes.
    pub save_mutex: Arc<tokio::sync::Mutex<()>>,
}

impl DeepAgent {
    pub fn new(
        harness: Harness, 
        state: DeepAgentState, 
        executor: Arc<dyn ToolExecutor>, 
        memory_controller: Arc<ResilientMemoryController<FileStorage>>,
        scheduler: Option<DreamScheduler<FileStorage>>
    ) -> Self {
        Self { 
            harness, 
            state, 
            executor, 
            memory_controller, 
            scheduler,
            save_mutex: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Executes a single reasoning/action step following the DeepAgent loop.
    #[tracing::instrument(skip(self, user_input), fields(session_id = %self.state.session_id, input_len = user_input.len()))]
    pub async fn step(&mut self, user_input: String) -> MentalistResult<String> {
        if let Some(s) = &self.scheduler {
            s.record_activity();
        }
        
        let mut full_content = String::new();
        // Use agent config from harness
        let agent_config = self.harness.config.agent.clone();
        let mut stream = Box::pin(self.step_stream(user_input, agent_config));
        
        while let Some(res) = stream.next().await {
            if let AgentStepEvent::TextChunk(c) = res? {
                full_content.push_str(&c);
            }
        }
        Ok(full_content)
    }

    /// Autonomous reasoning loop that executes tools and continues until a final answer is reached.
    #[tracing::instrument(skip(self, user_input, config), fields(session_id = %self.state.session_id))]
    pub fn step_stream(
        &mut self, 
        user_input: String,
        config: AgentConfig,
    ) -> BoxStream<'_, MentalistResult<AgentStepEvent>> {
        let stream = async_stream::try_stream! {
            // Explicitly state the chunk type to help inference
            if false { yield MentalistResult::Ok(AgentStepEvent::Status("".into()))?; }
            
            let start_time = std::time::Instant::now();

            // 1. Initial Prompt Handling - Add to Arc<Context>
            let current_context = Arc::make_mut(&mut self.state.context);
            current_context.items.push(mem_core::MemoryItem {
                role: mem_core::MemoryRole::User,
                content: user_input.clone(),
                timestamp: Utc::now().timestamp() as u64,
                metadata: serde_json::json!({}),
            });

            let mut turn_count = 0;

            loop {
                turn_count += 1;
                
                // Check context bounds and optimize if needed
                if self.state.context.items.len() > config.max_context_items {
                    yield AgentStepEvent::Status("Context limit reached. Optimizing...".into());
                    let mut current_ctx = (*self.state.context).clone();
                    if let Err(e) = self.harness.optimize_context(&mut current_ctx).await {
                        tracing::error!("Auto-optimization failed: {}", e);
                    } else {
                        self.state.context = Arc::new(current_ctx);
                    }
                }

                // Check turn limit
                if turn_count > config.max_turns {
                    let msg = format!("Turn limit ({}) reached. Stopping.", config.max_turns);
                    yield AgentStepEvent::Status(msg.clone());
                    if config.fail_on_limit {
                        Err(MentalistError::AgentError(msg))?;
                    }
                    break;
                }
                
                let mut tool_calls_this_turn = 0;

                // Check timeout
                if start_time.elapsed().as_secs() > config.timeout_seconds {
                    Err(MentalistError::AgentError(format!("Agent step timeout after {}s", config.timeout_seconds)))?;
                }

                // Fetch tools from the executor
                let tools = self.executor.list_tools().await.unwrap_or_default();
                
                let req = Request {
                    prompt: if turn_count == 1 { user_input.clone() } else { "Continue".to_string() },
                    context: self.state.context.clone(), // Cheap Arc clone
                    tools,
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
                            // Flush any previously buffered tool call before starting a new one.
                            // This supports providers that emit multiple tool calls as sequential
                            // ResponseChunks rather than a single delta stream.
                            if !current_tool_name.is_empty() {
                                let fixed_args = Self::fix_json(&current_tool_args);
                                match serde_json::from_str::<serde_json::Value>(&fixed_args) {
                                    Ok(arguments) => {
                                        tracing::debug!(tool = %current_tool_name, "Flushing intermediate tool call");
                                        tool_calls.push(ToolCall {
                                            name: std::mem::take(&mut current_tool_name),
                                            arguments,
                                        });
                                        current_tool_args.clear();
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, raw = %current_tool_args, "Failed to parse intermediate tool args; discarding");
                                        current_tool_name.clear();
                                        current_tool_args.clear();
                                    }
                                }
                            }
                            current_tool_name.push_str(&name);
                        }
                        if let Some(args) = delta.arguments_delta {
                            current_tool_args.push_str(&args);
                        }
                    }

                    // Flush the last buffered tool call on the final stream chunk.
                    if chunk.is_final && !current_tool_name.is_empty() {
                        let fixed_args = Self::fix_json(&current_tool_args);
                        let arguments: serde_json::Value = serde_json::from_str(&fixed_args)
                            .map_err(|e| {
                                tracing::error!(error = %e, raw = %current_tool_args, fixed = %fixed_args, "Failed to parse tool arguments");
                                MentalistError::AgentError(format!("Tool argument JSON parse error: {} for args: {}", e, fixed_args))
                            })?;
                        tool_calls.push(ToolCall { name: current_tool_name.clone(), arguments });
                        current_tool_args.clear();
                        current_tool_name.clear();
                    }
                }

                // FALLBACK: If no native tool_calls were detected from JSON deltas but the
                // accumulated text content contains a text-encoded tool call, attempt to parse
                // it. This is critical for small models (e.g. qwen2.5-coder:3b) that frequently
                // emit tool calls inside <tool_call> XML, ```json blocks, or as raw JSON objects
                // rather than via the structured tool_call_delta mechanism.
                if tool_calls.is_empty() && !final_content.is_empty() {
                    if let Some(fallback_call) = Self::parse_tool_call_from_text(&final_content) {
                        tracing::info!(tool_name = %fallback_call.name, "Text-encoded tool call detected via fallback parser");
                        // Remove the raw tool-call text from the response content so it
                        // doesn't appear as a visible message in the chat output.
                        final_content.clear();
                        tool_calls.push(fallback_call);
                    }
                }

                if tool_calls.is_empty() {
                    let current_context = Arc::make_mut(&mut self.state.context);
                    current_context.items.push(mem_core::MemoryItem {
                        role: mem_core::MemoryRole::Assistant,
                        content: final_content,
                        timestamp: Utc::now().timestamp() as u64,
                        metadata: serde_json::json!({}),
                    });
                    break;
                }

                for mut tool in tool_calls {
                    tool_calls_this_turn += 1;
                    if tool_calls_this_turn > config.max_tool_calls_per_turn {
                        let msg = format!("Security: Infinite tool chain detected (> {} calls in one turn). Aborting turn.", config.max_tool_calls_per_turn);
                        tracing::error!(msg);
                        yield AgentStepEvent::Status("Security Alert: Infinite Tool Cycle Detected".into());
                        break; // Stop processing tools and reasoning
                    }

                    let tool_name = tool.name.clone();
                    yield AgentStepEvent::ToolStarted(tool_name.clone());
                    
                    self.harness.run_before_tool_hooks(&mut tool).await?;
                    
                    let mut retry_count = 0;
                    let result = loop {
                        match self.executor.execute(&tool_name, tool.arguments.clone()).await {
                            Ok(res) => break Ok(res),
                            Err(e) => {
                                let err_msg = format!("Tool error: {}", e);
                                
                                // Categorize error using structured variants
                                let error_category = if let Some(te) = e.downcast_ref::<crate::executor::ToolError>() {
                                    match te {
                                        crate::executor::ToolError::Transient(_) => "transient_timeout",
                                        crate::executor::ToolError::NotFound(_) => "tool_not_found",
                                        crate::executor::ToolError::PermissionDenied(_) => "permission_denied",
                                        crate::executor::ToolError::ExecutionFailed(_) => "execution_failed",
                                        crate::executor::ToolError::ResourceLimitExceeded(_) => "resource_limit",
                                        crate::executor::ToolError::SecurityViolation(_) => "security_violation",
                                        _ => "internal_error",
                                    }
                                } else {
                                    let error_text = e.to_string().to_lowercase();
                                    match error_text.as_str() {
                                        s if s.contains("timeout") => "transient_timeout",
                                        s if s.contains("not found") => "tool_not_found",
                                        s if s.contains("permission") || s.contains("denied") => "permission_denied",
                                        _ => "unknown",
                                    }
                                };

                                if error_category == "transient_timeout" && retry_count < config.max_retries {
                                    retry_count += 1;
                                    let backoff = 2u64.pow(retry_count as u32);
                                    let msg = format!("Transient error detected ({}). Retrying in {}s (Attempt {}/{})", error_category, backoff, retry_count, config.max_retries);
                                    yield AgentStepEvent::Status(msg);
                                    tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                                    continue;
                                }
                                
                                break Err((err_msg, error_category.to_string()));
                            }
                        }
                    };

                    match result {
                        Ok(mut result) => {
                            self.harness.run_after_tool_hooks(&tool, &mut result).await?;
                            yield AgentStepEvent::ToolFinished(tool_name.clone(), result.clone());
                            
                            let current_ctx = Arc::make_mut(&mut self.state.context);
                            current_ctx.items.push(mem_core::MemoryItem {
                                role: mem_core::MemoryRole::Tool,
                                content: result,
                                timestamp: Utc::now().timestamp() as u64,
                                metadata: serde_json::json!({"tool": tool_name}),
                            });
                        }
                        Err((err_msg, error_category)) => {
                            yield AgentStepEvent::Status(err_msg.clone());
                            
                            let current_ctx = Arc::make_mut(&mut self.state.context);
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
                        }
                    }
                }
            }

            let _ = self.save_state_resilient().await;
        };
        Box::pin(stream)
    }

    /// Persists agent state atomically using temp files.
    #[tracing::instrument(skip(self), fields(session_id = %self.state.session_id))]
    pub async fn save_state_resilient(&mut self) -> MentalistResult<()> {
        let _guard = self.save_mutex.lock().await;
        // Use a hidden directory for internal state if possible, but keep .agent for backwards compatibility
        let root = PathBuf::from(".agent/sessions");
        
        // Atomic directory creation
        tokio::fs::create_dir_all(&root).await.map_err(|e| MentalistError::Internal(format!("Failed to create session dir: {}", e)))?;
        
        let path = root.join(format!("session_{}.session", self.state.session_id));
        
        let mut optimized_context = (*self.state.context).clone();
        self.memory_controller.optimize_resilient(&mut optimized_context).await
            .map_err(|e| MentalistError::Internal(format!("Context optimization failed: {}", e)))?;
        
        let optimized_arc = Arc::new(optimized_context);
        self.state.context = optimized_arc.clone();
        
        // Atomic write: write to temp file, then rename
        let data = serde_json::to_vec_pretty(&self.state).map_err(|e| MentalistError::Internal(format!("Serialization error: {}", e)))?;
        let temp_file = root.join(format!(".session_{}.tmp", self.state.session_id));
        tokio::fs::write(&temp_file, data).await.map_err(|e| MentalistError::Internal(format!("Write error: {}", e)))?;
        tokio::fs::rename(&temp_file, path).await.map_err(|e| MentalistError::Internal(format!("Rename error: {}", e)))?;
        
        Ok(())
    }

    /// Simple heuristic fixer for malformed JSON deltas
    fn fix_json(s: &str) -> String {
        let s = s.trim();
        if s.is_empty() { return "{}".into(); }
        
        let mut fixed = s.to_string();
        
        // Remove trailing comma which models often leave before closing
        if fixed.ends_with(',') {
            fixed.pop();
        }

        // Close unclosed structures
        let open_braces = fixed.chars().filter(|&c| c == '{').count();
        let close_braces = fixed.chars().filter(|&c| c == '}').count();
        if open_braces > close_braces {
            fixed.push_str(&"}".repeat(open_braces - close_braces));
        }
        
        let open_brackets = fixed.chars().filter(|&c| c == '[').count();
        let close_brackets = fixed.chars().filter(|&c| c == ']').count();
        if open_brackets > close_brackets {
            fixed.push_str(&"]".repeat(open_brackets - close_brackets));
        }

        fixed
    }

    /// Attempts to extract a single tool call from text content using multiple fallback
    /// strategies. Used when the model emits tool calls as formatted text rather than
    /// via the native `tool_calls` JSON field in the provider response.
    ///
    /// Strategies (in priority order):
    /// 1. XML-style `<tool_call>{...}</tool_call>` tags
    /// 2. Fenced ` ```json ... ``` ` or ` ``` ... ``` ` code blocks
    /// 3. Raw JSON object at the top level of the content string
    fn parse_tool_call_from_text(content: &str) -> Option<ToolCall> {
        // 1. Try XML-like <tool_call>...</tool_call>
        if let Some(start) = content.find("<tool_call>") {
            if let Some(end_offset) = content[start..].find("</tool_call>") {
                let json_str = &content[start + 11..start + end_offset];
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if let (Some(name), Some(args)) = (
                        val["name"].as_str(),
                        val["arguments"].as_object(),
                    ) {
                        return Some(ToolCall {
                            name: name.to_string(),
                            arguments: serde_json::Value::Object(args.clone()),
                        });
                    }
                }
            }
        }

        // 2. Try fenced ```json ... ``` or ``` ... ``` code blocks
        if let Some(start) = content.find("```json") {
            let search = &content[start + 7..];
            if let Some(end) = search.find("```") {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(search[..end].trim()) {
                    if let (Some(name), Some(args)) = (
                        val["name"].as_str(),
                        val["arguments"].as_object(),
                    ) {
                        return Some(ToolCall {
                            name: name.to_string(),
                            arguments: serde_json::Value::Object(args.clone()),
                        });
                    }
                }
            }
        } else if let Some(start) = content.find("```") {
            let search = &content[start + 3..];
            if let Some(end) = search.find("```") {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(search[..end].trim()) {
                    if let (Some(name), Some(args)) = (
                        val["name"].as_str(),
                        val["arguments"].as_object(),
                    ) {
                        return Some(ToolCall {
                            name: name.to_string(),
                            arguments: serde_json::Value::Object(args.clone()),
                        });
                    }
                }
            }
        }

        // 3. Try parsing the entire content as a raw JSON object
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(content.trim()) {
            if let (Some(name), Some(args)) = (
                val["name"].as_str(),
                val["arguments"].as_object(),
            ) {
                return Some(ToolCall {
                    name: name.to_string(),
                    arguments: serde_json::Value::Object(args.clone()),
                });
            }
        }

        None
    }
}


pub enum AgentStepEvent {
    TextChunk(String),
    ToolStarted(String),
    ToolFinished(String, String),
    Status(String),
}
