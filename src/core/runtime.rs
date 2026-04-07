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
        input_tokens: usize,
        output_tokens: usize,
        context_size: usize,
    },
    AwaitingApproval(mem_planner::ExecutionPlan),
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
        tx: Option<UnboundedSender<RuntimeEvent>>,
        mut approval_rx: Option<tokio::sync::mpsc::Receiver<bool>>
    ) -> anyhow::Result<String> {
        let mut ctx = ctx; 
        let mut step = 0;
        let mut total_input_tokens = 0;
        let mut total_output_tokens = 0;
        let mut completed = false;

        let send_metrics = |step: usize, phase: &str, tx: &Option<UnboundedSender<RuntimeEvent>>, input: usize, output: usize, ctx: &Context| {
            if let Some(ref tx) = tx {
                let context_size = ctx.items.iter().map(|i| mem_core::estimate_tokens(&i.content)).sum();
                let _ = tx.send(RuntimeEvent::MetricUpdate { 
                    step, 
                    phase: phase.into(), 
                    input_tokens: input, 
                    output_tokens: output, 
                    context_size 
                });
            }
        };

        while step < self.limits.max_steps && !completed {
            step += 1;
            
            send_metrics(step, "PLAN", &tx, total_input_tokens, total_output_tokens, &ctx);
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::Status(format!("Step {}: Planning...", step)));
            }
            
            // 0. SUMMARIZE CONTEXT (if needed)
            let current_tokens: usize = ctx.items.iter().map(|i| mem_core::estimate_tokens(&i.content)).sum();
            if current_tokens > 24000 && ctx.items.len() > 10 {
                tracing::info!(current_tokens, "Context threshold exceeded, summarization triggered");
                if let Some(ref tx) = tx {
                    let _ = tx.send(RuntimeEvent::Status("Deep memory compression...".into()));
                }
                ctx = self.summarize_context(ctx).await?;
            }
            
            tracing::info!(step, goal, "Phase: PLAN");
            
            // 1. PLAN
            let tools = self.tools.list_tools().await;
            let mut plan = self.planner.create_plan(goal, &ctx, tools, None).await?;
            if let Some(usage) = &plan.usage {
                total_input_tokens += usage.prompt_tokens as usize;
                total_output_tokens += usage.completion_tokens as usize;
            }
            tracing::info!(tasks = ?plan.tasks.keys().collect::<Vec<_>>(), "Initial plan");

            if plan.requires_approval {
                if let Some(ref tx) = tx {
                    let _ = tx.send(RuntimeEvent::AwaitingApproval(plan.clone()));
                }
                if let Some(ref mut rx) = approval_rx {
                    match rx.recv().await {
                        Some(true) => tracing::info!("Plan approved by user"),
                        _ => {
                            tracing::info!("Plan rejected or channel closed");
                            completed = true;
                            continue;
                        }
                    }
                } else {
                    tracing::warn!("Plan requires approval but no approval_rx provided. Auto-approving for headless mode.");
                }
            }

            if !plan.content.is_empty() {
                if let Some(ref tx) = tx {
                    let _ = tx.send(RuntimeEvent::TextChunk(plan.content.clone()));
                }
            }
            
            // Fallback & Conversational logic
            if plan.tasks.is_empty() {
                // 1. Try from plan.content (raw LLM response for current step)
                let fallback_nodes = self.parse_fallback_tool_calls(&plan.content);
                if !fallback_nodes.is_empty() {
                    tracing::info!(step, "Using fallback parser: {} tools found in plan.content", fallback_nodes.len());
                    for node in fallback_nodes {
                        plan.tasks.insert(node.id.clone(), node);
                    }
                } else if let Some(last_msg) = ctx.items.last() {
                    // 2. Try from last message in context (e.g. if the user provided multiple tool calls)
                    let fallback_nodes = self.parse_fallback_tool_calls(&last_msg.content);
                    if !fallback_nodes.is_empty() {
                        tracing::info!(step, "Using fallback parser: {} tools found in last context message", fallback_nodes.len());
                        for node in fallback_nodes {
                            plan.tasks.insert(node.id.clone(), node);
                        }
                    }
                }
                
                // 3. Still empty? If we have content, it's conversational
                if plan.tasks.is_empty() && !plan.content.is_empty() {
                    send_metrics(step, "PLAN_COMPLETE", &tx, total_input_tokens, total_output_tokens, &ctx);
                    completed = true;
                    continue; // Skip execution phase
                }
            }
            
            if plan.tasks.is_empty() {
                tracing::warn!("Execution plan is empty and no fallback tool calls found for goal: '{}'", goal);
                completed = true;
                continue;
            }

            // 2. EXECUTE
            send_metrics(step, "EXECUTE", &tx, total_input_tokens, total_output_tokens, &ctx);
            if let Some(ref tx) = tx {
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
            send_metrics(step, "CRITIQUE", &tx, total_input_tokens, total_output_tokens, &ctx);
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::Status(format!("Step {}: Critiquing results...", step)));
            }
            
            tracing::info!(step, "Phase: CRITIQUE");
            let feedback = self.critic.evaluate(goal, &ctx, &results).await?;
            
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::Status(format!("Critic: {}", feedback.critique)));
            }
            
            // 4. STORE MEMORY
            send_metrics(step, "STORE", &tx, total_input_tokens, total_output_tokens, &ctx);
            
            tracing::info!(step, "Phase: STORE MEMORY");
            for (_, res) in results {
                self.memory.store(crate::memory::MemoryEvent {
                    content: format!("Task result: {:?}", res.output),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    metadata: serde_json::json!({ "task_id": res.task_id }),
                }).await?;
            }

            // 5. ADAPT
            send_metrics(step, "ADAPT", &tx, total_input_tokens, total_output_tokens, &ctx);
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::Status(format!("Step {}: Adapting plan...", step)));
            }
            
            tracing::info!(step, feedback = feedback.critique, "Phase: ADAPT");
            if feedback.score > 0.9 && !feedback.suggests_retry {
                completed = true;
            } else if feedback.suggests_retry {
                // If the critic suggests retry, we re-plan with tools
                let tools = self.tools.list_tools().await;
                plan = self.planner.create_plan(goal, &ctx, tools, Some(&feedback.critique)).await?;
                tracing::info!("Re-planned due to critic feedback");
            }
        }

        // 6. FINAL SUMMARY
        if let Some(ref tx) = tx {
            let _ = tx.send(RuntimeEvent::Status("Finalizing report...".into()));
        }
        let tools = self.tools.list_tools().await;
        let summary_plan = self.planner.create_plan(
            &format!("Summarize the results of our steps to satisfy the original goal: {}", goal),
            &ctx,
            tools,
            None
        ).await?;
        
        if !summary_plan.content.is_empty() {
             if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::TextChunk(summary_plan.content));
            }
        }

        send_metrics(step, "COMPLETED", &tx, total_input_tokens, total_output_tokens, &ctx);
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
            let tools = self.tools.list_tools().await;
            let plan_res = self.planner.create_plan(&goal, &ctx, tools, None).await;
            let mut plan = match plan_res {
                Ok(p) => p,
                Err(e) => return Some((Err(e), (step, true, ctx, goal))),
            };

            // Fallback & Conversational
            if plan.tasks.is_empty() {
                let fallback_nodes = self.parse_fallback_tool_calls(&plan.content);
                if !fallback_nodes.is_empty() {
                    for node in fallback_nodes {
                        plan.add_task(node);
                    }
                } else if let Some(last_msg) = ctx.items.last() {
                    let fallback_nodes = self.parse_fallback_tool_calls(&last_msg.content);
                    for node in fallback_nodes {
                        plan.add_task(node);
                    }
                }
            }

            if plan.tasks.is_empty() {
                if !plan.content.is_empty() {
                    return Some((Ok(RuntimeEvent::TextChunk(plan.content)), (step, true, ctx, goal)));
                }
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
            let feedback_res = self.critic.evaluate(&goal, &ctx, &results).await;
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

    pub async fn summarize_context(&self, mut ctx: Context) -> anyhow::Result<Context> {
        // Keep the goal and the last 4 messages intact
        if ctx.items.len() <= 6 {
            return Ok(ctx);
        }

        let to_summarize = &ctx.items[..ctx.items.len() - 4];
        let history_text = to_summarize.iter()
            .map(|i| format!("{:?}: {}", i.role, i.content))
            .collect::<Vec<_>>()
            .join("\n---\n");

        let prompt = format!(
            r#"Summarize the following conversation history into a dense, informative paragraph.
Preserve all key facts, discovered information, and completed task results.

### HISTORY ###
{}

### SUMMARY ###
"#,
            history_text
        );

        let req = crate::llm::LlmRequest {
            prompt,
            context: std::sync::Arc::new(Context::default()),
            tools: vec![],
        };

        let response = self.llm.generate(req).await?;
        
        let mut new_items = vec![mem_core::MemoryItem {
            role: mem_core::MemoryRole::System,
            content: format!("### PREVIOUS CONTEXT SUMMARY ###\n{}", response.content),
            timestamp: chrono::Utc::now().timestamp() as u64,
            metadata: serde_json::json!({ "summarized": true }),
        }];

        new_items.extend(ctx.items.drain(ctx.items.len() - 4..));
        ctx.items = new_items;

        Ok(ctx)
    }
}
