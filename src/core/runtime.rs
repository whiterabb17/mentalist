use std::sync::Arc;
use mem_core::Context;
use crate::cognition::{Planner, Critic};
use crate::execution::executor::Executor;
use crate::memory::MemoryStore;
use crate::llm::LLMProvider;
use crate::tools::ToolRegistry;
use crate::security::SecurityEngine;

pub struct ExecutionLimits {
    pub max_steps: usize,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Status(String),
    TextChunk(String),
    ToolStarted(String),
    ToolFinished(String, String, bool), // name, result, success
    MetricUpdate {
        step: usize,
        phase: String,
    },
}

pub struct AgentRuntime {
    pub planner: Arc<dyn Planner>,
    pub executor: Arc<Executor>,
    pub memory: Arc<dyn MemoryStore>,
    pub llm: Arc<dyn LLMProvider>,
    pub tools: Arc<ToolRegistry>,
    pub security: Arc<SecurityEngine>,
    pub critic: Arc<dyn Critic>,
    pub limits: ExecutionLimits,
}

impl AgentRuntime {
    pub async fn run(&self, goal: &str, ctx: Context) -> anyhow::Result<String> {
        let mut step = 0;
        let mut completed = false;

        while step < self.limits.max_steps && !completed {
            step += 1;
            tracing::info!(step, goal, "Phase: PLAN");
            
            // 1. PLAN
            let mut plan = self.planner.create_plan(goal, &ctx, None).await?;
            
            // Fallback: If plan is empty, try parsing tools from the last message in context
            if plan.tasks.is_empty() {
                if let Some(last_msg) = ctx.items.last() {
                    let fallback_nodes = self.parse_fallback_tool_calls(&last_msg.content);
                    if !fallback_nodes.is_empty() {
                        tracing::info!(step, "Using fallback parser results: {} tools found", fallback_nodes.len());
                        for node in fallback_nodes {
                            plan.tasks.insert(node.id.clone(), node);
                        }
                    }
                }
            }

            // 2. EXECUTE
            tracing::info!(step, "Phase: EXECUTE");
            let graph = crate::execution::graph::TaskGraph::new(plan);
            let tools = Arc::clone(&self.tools);
            let security = Arc::clone(&self.security);

            let results = self.executor.execute_parallel(&graph, move |task| {
                let tools = Arc::clone(&tools);
                let security = Arc::clone(&security);
                async move {
                    let mut success = false;
                    let mut output = serde_json::Value::String("Task failed: No tool specified".into());

                    if let Some(tool_name) = task.tool_name {
                        if let Err(e) = security.validate_tool_call(&tool_name) {
                            output = serde_json::Value::String(format!("Security Violation: {}", e));
                        } else if let Some(tool) = tools.get(&tool_name).await {
                            let args = task.tool_args.unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                            match tool.execute(args).await {
                                Ok(res) => {
                                    output = res;
                                    success = true;
                                }
                                Err(e) => {
                                    output = serde_json::Value::String(format!("Execution Error: {}", e));
                                }
                            }
                        } else {
                            output = serde_json::Value::String(format!("Tool '{}' not found", tool_name));
                        }
                    }

                    crate::execution::executor::ExecutionResult {
                        task_id: task.id,
                        output,
                        success,
                    }
                }
            }).await?;

            // 3. CRITIQUE
            tracing::info!(step, "Phase: CRITIQUE");
            let feedback = self.critic.evaluate(&results).await?;
            
            // 4. STORE MEMORY
            tracing::info!(step, "Phase: STORE MEMORY");
            for (_, res) in results {
                self.memory.store(crate::memory::MemoryEvent {
                    content: format!("Task result: {:?}", res.output),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    metadata: serde_json::json!({ "task_id": res.task_id }),
                }).await?;
            }

            // 5. ADAPT
            tracing::info!(step, feedback = feedback.critique, "Phase: ADAPT");
            if feedback.score > 0.9 && !feedback.suggests_retry {
                completed = true;
            }
        }

        Ok("Goal completed".into())
    }

    pub fn run_stream(
        &self, 
        goal: String, 
        ctx: Context
    ) -> impl futures_util::Stream<Item = anyhow::Result<RuntimeEvent>> + '_ {
        use futures_util::stream;
        
        stream::unfold((0, false, ctx, goal), move |(mut step, mut completed, mut ctx, goal)| async move {
            if step >= self.limits.max_steps || completed {
                return None;
            }
            step += 1;

            // Simple wrapper to yield multiple events for one step if needed
            // For a mature implementation, this would be a more complex state machine
            
            // 1. PLAN
            let plan_res = self.planner.create_plan(&goal, &ctx, None).await;
            let mut plan = match plan_res {
                Ok(p) => p,
                Err(e) => return Some((Err(e), (step, true, ctx))),
            };

            // Fallback
            if plan.tasks.is_empty() {
                if let Some(last_msg) = ctx.items.last() {
                    let fallback_nodes = self.parse_fallback_tool_calls(&last_msg.content);
                    for node in fallback_nodes {
                        plan.add_task(node);
                    }
                }
            }

            if plan.tasks.is_empty() {
                return Some((Ok(RuntimeEvent::Status("No more tasks planned.".into())), (step, true, ctx, goal)));
            }

            // 2. EXECUTE (We'll yield tool events)
            let graph = crate::execution::graph::TaskGraph::new(plan);
            let tools = Arc::clone(&self.tools);
            let security = Arc::clone(&self.security);

            let results_res = self.executor.execute_parallel(&graph, move |task| {
                let tools = Arc::clone(&tools);
                let security = Arc::clone(&security);
                async move {
                    let mut success = false;
                    let mut output = serde_json::Value::String("Task failed: No tool specified".into());

                    if let Some(tool_name) = task.tool_name {
                        if let Err(e) = security.validate_tool_call(&tool_name) {
                            output = serde_json::Value::String(format!("Security Violation: {}", e));
                        } else if let Some(tool) = tools.get(&tool_name).await {
                            let args = task.tool_args.unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                            match tool.execute(args).await {
                                Ok(res) => {
                                    output = res;
                                    success = true;
                                }
                                Err(e) => {
                                    output = serde_json::Value::String(format!("Execution Error: {}", e));
                                }
                            }
                        }
                    }

                    crate::execution::executor::ExecutionResult {
                        task_id: task.id,
                        output,
                        success,
                    }
                }
            }).await;

            let results = match results_res {
                Ok(r) => r,
                Err(e) => return Some((Err(e), (step, true, ctx))),
            };

            // 3. CRITIQUE
            let feedback_res = self.critic.evaluate(&results).await;
            let feedback = match feedback_res {
                Ok(f) => f,
                Err(e) => return Some((Err(e), (step, true, ctx))),
            };

            if feedback.score > 0.9 && !feedback.suggests_retry {
                completed = true;
            }

            // Update context (Side effects in mentalist usually handled by the caller or middleware)
            // For this stream, we just yield a status
            Some((Ok(RuntimeEvent::Status(format!("Step {} complete: {}", step, feedback.critique))), (step, completed, ctx, goal)))
        })
    }

    /// Fallback parser for tool calls embedded in raw text (XML tags, code blocks, or raw JSON).
    /// Used when the model fails to emit structured tool call JSON but provides text-based intent.
    pub fn parse_fallback_tool_calls(&self, content: &str) -> Vec<mem_planner::TaskNode> {
        let mut tasks = Vec::new();
        
        // 1. XML-style fallback: <call name="tool_name">{"arg":"val"}</call>
        let xml_regex = regex::Regex::new(r#"<call\s+name="([^"]+)"\s*>(.*?)</call>"#).ok();
        if let Some(re) = xml_regex {
            for cap in re.captures_iter(content) {
                let name = cap[1].to_string();
                let args_str = cap[2].trim();
                let args = serde_json::from_str(args_str).unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                let id = mem_planner::TaskId(format!("fallback_{}", tasks.len()));
                tasks.push(mem_planner::TaskNode {
                    id,
                    name: format!("Fallback: {}", name),
                    description: format!("Extracted from raw text: {}", name),
                    tool_name: Some(name),
                    tool_args: Some(args),
                    dependencies: Vec::new(),
                    metadata: serde_json::json!({ "fallback": true }),
                });
            }
        }

        // 2. Fenced code block fallback: ```tool:name ... ```
        if tasks.is_empty() {
            let fence_regex = regex::Regex::new(r"```tool:([a-zA-Z0-9_-]+)\s*\n(.*?)\n```").ok();
            if let Some(re) = fence_regex {
                for cap in re.captures_iter(content) {
                    let name = cap[1].to_string();
                    let args = serde_json::from_str(cap[2].trim()).unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                    let id = mem_planner::TaskId(format!("fallback_{}", tasks.len()));
                    tasks.push(mem_planner::TaskNode {
                        id,
                        name: format!("Fallback: {}", name),
                        description: format!("Extracted from code block: {}", name),
                        tool_name: Some(name),
                        tool_args: Some(args),
                        dependencies: Vec::new(),
                        metadata: serde_json::json!({ "fallback": true }),
                    });
                }
            }
        }

        // 3. Raw JSON object fallback: {"tool": "name", "arguments": {}}
        if tasks.is_empty() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(content.trim()) {
                if let Some(obj) = val.as_object() {
                    let name = obj.get("tool").or_else(|| obj.get("name")).and_then(|v| v.as_str());
                    let args = obj.get("arguments").or_else(|| obj.get("args")).cloned();
                    if let Some(name) = name {
                        let id = mem_planner::TaskId(format!("fallback_json_{}", tasks.len()));
                        tasks.push(mem_planner::TaskNode {
                            id,
                            name: format!("Fallback: {}", name),
                            description: format!("Extracted from raw JSON: {}", name),
                            tool_name: Some(name.to_string()),
                            tool_args: args,
                            dependencies: Vec::new(),
                            metadata: serde_json::json!({ "fallback": true }),
                        });
                    }
                }
            }
        }

        tasks
    }
}
