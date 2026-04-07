use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
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
    pub async fn run(
        &self, 
        goal: &str, 
        ctx: Context, 
        tx: Option<UnboundedSender<RuntimeEvent>>
    ) -> anyhow::Result<String> {
        let mut step = 0;
        let mut completed = false;

        while step < self.limits.max_steps && !completed {
            step += 1;
            
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::MetricUpdate { step, phase: "PLAN".into() });
                let _ = tx.send(RuntimeEvent::Status(format!("Step {}: Planning...", step)));
            }
            
            tracing::info!(step, goal, "Phase: PLAN");
            
            // 1. PLAN
            let mut plan = self.planner.create_plan(goal, &ctx, None).await?;
            
            // Fallback
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
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::MetricUpdate { step, phase: "EXECUTE".into() });
                let _ = tx.send(RuntimeEvent::Status(format!("Step {}: Executing tasks...", step)));
            }
            
            tracing::info!(step, "Phase: EXECUTE");
            let graph = crate::execution::graph::TaskGraph::new(plan);
            let tools = Arc::clone(&self.tools);
            let security = Arc::clone(&self.security);
            let tx_inner = tx.clone();

            let results = self.executor.execute_parallel(&graph, move |task| {
                let tools = Arc::clone(&tools);
                let security = Arc::clone(&security);
                let tx_deep = tx_inner.clone();
                async move {
                    let mut success = false;
                    let mut output = serde_json::Value::String("Task failed: No tool specified".into());

                    if let Some(tool_name) = task.tool_name {
                        if let Some(ref tx) = tx_deep {
                            let _ = tx.send(RuntimeEvent::ToolStarted(tool_name.clone()));
                        }
                        
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
                        
                        if let Some(ref tx) = tx_deep {
                            let _ = tx.send(RuntimeEvent::ToolFinished(tool_name, output.to_string(), success));
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
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::MetricUpdate { step, phase: "CRITIQUE".into() });
                let _ = tx.send(RuntimeEvent::Status(format!("Step {}: Critiquing results...", step)));
            }
            
            tracing::info!(step, "Phase: CRITIQUE");
            let feedback = self.critic.evaluate(&results).await?;
            
            // 4. STORE MEMORY
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::MetricUpdate { step, phase: "STORE".into() });
            }
            
            tracing::info!(step, "Phase: STORE MEMORY");
            for (_, res) in results {
                self.memory.store(crate::memory::MemoryEvent {
                    content: format!("Task result: {:?}", res.output),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    metadata: serde_json::json!({ "task_id": res.task_id }),
                }).await?;
            }

            // 5. ADAPT
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::MetricUpdate { step, phase: "ADAPT".into() });
                let _ = tx.send(RuntimeEvent::Status(format!("Step {}: Adapting plan...", step)));
            }
            
            tracing::info!(step, feedback = feedback.critique, "Phase: ADAPT");
            if feedback.score > 0.9 && !feedback.suggests_retry {
                completed = true;
            }
        }

        if let Some(ref tx) = tx {
            let _ = tx.send(RuntimeEvent::Status("Goal completed.".into()));
        }

        Ok("Goal completed".into())
    }

    pub fn run_stream(
        &self, 
        goal: String, 
        ctx: Context
    ) -> impl futures_util::Stream<Item = anyhow::Result<RuntimeEvent>> + '_ {
        use futures_util::stream;
        
        stream::unfold((0, false, ctx, goal), move |(mut step, mut completed, ctx, goal)| async move {
            if step >= self.limits.max_steps || completed {
                return None;
            }
            step += 1;

            // 1. PLAN
            let plan_res = self.planner.create_plan(&goal, &ctx, None).await;
            let mut plan = match plan_res {
                Ok(p) => p,
                Err(e) => return Some((Err(e), (step, true, ctx, goal))),
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

            // 2. EXECUTE
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
                Err(e) => return Some((Err(e), (step, true, ctx, goal))),
            };

            // 3. CRITIQUE
            let feedback_res = self.critic.evaluate(&results).await;
            let feedback = match feedback_res {
                Ok(f) => f,
                Err(e) => return Some((Err(e), (step, true, ctx, goal))),
            };

            if feedback.score > 0.9 && !feedback.suggests_retry {
                completed = true;
            }

            Some((Ok(RuntimeEvent::Status(format!("Step {} complete: {}", step, feedback.critique))), (step, completed, ctx, goal)))
        })
    }

    pub fn parse_fallback_tool_calls(&self, content: &str) -> Vec<mem_planner::TaskNode> {
        let mut tasks = Vec::new();
        
        // XML-style fallback
        let xml_regex = regex::Regex::new(r#"<call\s+name="([^"]+)"\s*>(.*?)</call>"#).ok();
        if let Some(re) = xml_regex {
            for cap in re.captures_iter(content) {
                let name = cap[1].to_string();
                let args_str = cap[2].trim();
                let args = serde_json::from_str(args_str).unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                let id = mem_planner::TaskId::new();
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

        // Fenced code block fallback
        if tasks.is_empty() {
            let fence_regex = regex::Regex::new(r"```tool:([a-zA-Z0-9_-]+)\s*\n(.*?)\n```").ok();
            if let Some(re) = fence_regex {
                for cap in re.captures_iter(content) {
                    let name = cap[1].to_string();
                    let args = serde_json::from_str(cap[2].trim()).unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                    let id = mem_planner::TaskId::new();
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

        tasks
    }
}
