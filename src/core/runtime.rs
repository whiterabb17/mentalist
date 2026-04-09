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
    pub middlewares: Vec<Arc<dyn crate::middleware::Middleware>>,
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
        let mut critique: Option<String> = None;
        let mut last_plan_content = String::new();

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

        // 0. SECURITY: SANITIZE GOAL
        let goal = self.security.sanitize_prompt(goal);
        let goal = &goal;

        while step < self.limits.max_steps && !completed {
            step += 1;
            
            send_metrics(step, "PLAN", &tx, total_input_tokens, total_output_tokens, &ctx);
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::Status(format!("Step {}: Planning...", step)));
            }

            // 1. PROACTIVE RAG (Middleware Hook)
            let mut req = mem_core::Request {
                prompt: goal.to_string(),
                context: Arc::new(ctx.clone()),
                tools: Vec::new(), // Populated by planner or ToolDiscoveryMiddleware
            };

            for mw in &self.middlewares {
                if let Err(e) = mw.before_ai_call(&mut req).await {
                    tracing::error!(mw = mw.name(), error = ?e, "Middleware before_ai_call failed");
                    if mw.is_critical() { return Err(e); }
                }
            }
            // Update context from enriched request (RAG facts injected here)
            ctx = (*req.context).clone();
            
            // 2. SUMMARIZE CONTEXT (if needed)
            let current_tokens: usize = ctx.items.iter().map(|i| mem_core::estimate_tokens(&i.content)).sum();
            if current_tokens > 24000 && ctx.items.len() > 10 {
                tracing::info!(current_tokens, "Context threshold exceeded, summarization triggered");
                if let Some(ref tx) = tx {
                    let _ = tx.send(RuntimeEvent::Status("Deep memory compression...".into()));
                }
                ctx = self.summarize_context(ctx).await?;
            }
            
            tracing::info!(step, goal, "Phase: PLAN");
            
            // 3. PLAN
            let tools = self.tools.list_tools().await;
            let mut plan = self.planner.create_plan(goal, &ctx, tools, critique.take().as_deref()).await?;
            last_plan_content = plan.content.clone();
            if let Some(usage) = &plan.usage {
                total_input_tokens += usage.prompt_tokens as usize;
                total_output_tokens += usage.completion_tokens as usize;
            }
            
            // 4. DEDUCTIVE FACT EXTRACTION (Middleware Hook)
            let mut res_mw = mem_core::Response {
                content: plan.content.clone(),
                tool_calls: Vec::new(), // Not used for extraction in the planner phase usually, but hooks are there
                usage: plan.usage.clone(),
            };
            for mw in &self.middlewares {
                if let Err(e) = mw.after_ai_call(&mut res_mw).await {
                    tracing::error!(mw = mw.name(), error = ?e, "Middleware after_ai_call failed");
                }
            }
            
            let task_info: Vec<String> = plan.tasks.values().map(|t| format!("{} ({})", t.name, t.tool_name.as_deref().unwrap_or("no-tool"))).collect();
            tracing::info!(tasks = ?task_info, "Initial plan");

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
                    if plan.content == mem_planner::PLAN_BOILERPLATE_TASKS || plan.content == mem_planner::PLAN_BOILERPLATE_NO_TASKS {
                        let _ = tx.send(RuntimeEvent::Status(plan.content.clone()));
                    } else {
                        let _ = tx.send(RuntimeEvent::TextChunk(plan.content.clone()));
                    }
                }
            }
            
            // 4. PLAN VALIDATION & RECOVERY
            let is_json = plan.content.trim().starts_with('{') || plan.content.contains("\"tasks\":");
            
            // If tasks are missing, always try fallback parsing (handles XML, JSON fragments, etc.)
            if plan.tasks.is_empty() {
                let fallback_nodes = self.parse_fallback_tool_calls(&plan.content);
                if !fallback_nodes.is_empty() {
                    tracing::info!(step, "Recovered {} tasks using fallback parser", fallback_nodes.len());
                    for node in fallback_nodes {
                        plan.tasks.insert(node.id.clone(), node);
                    }
                }
            }

            // Still empty?
            if plan.tasks.is_empty() {
                // If it looks like JSON but we couldn't recover, it's a hard failure
                if is_json {
                     anyhow::bail!("Failed to parse execution plan format from LLM. Raw content: {}", plan.content);
                }
                
                // If there's natural language content, it's a conversational response
                if !plan.content.is_empty() {
                    if let Some(ref tx) = tx {
                        let _ = tx.send(RuntimeEvent::Status("Goal completed.".into()));
                    }
                    return Ok(plan.content);
                }

                // Completely empty?
                anyhow::bail!("No actions or response generated for goal at step {}", step);
            }

            // 5. EXECUTE
            send_metrics(step, "EXECUTE", &tx, total_input_tokens, total_output_tokens, &ctx);
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::Status(format!("Step {}: Executing tasks...", step)));
            }
            
            tracing::info!(step, "Phase: EXECUTE");
            let graph = crate::execution::graph::TaskGraph::new(plan.clone());
            let tools = Arc::clone(&self.tools);
            let security = Arc::clone(&self.security);
            let tx_inner = tx.clone();

            let mws = self.middlewares.clone();
            let results = self.executor.execute_parallel(&graph, move |task| {
                let tools = Arc::clone(&tools);
                let security = Arc::clone(&security);
                let tx_deep = tx_inner.clone();
                let mws = mws.clone(); 
                async move {
                    let mut success = false;
                    let mut output = serde_json::Value::String("Task failed: No tool specified".into());

                    if let Some(tool_name) = task.tool_name {
                        if let Some(ref tx) = tx_deep {
                            let _ = tx.send(RuntimeEvent::ToolStarted(tool_name.clone()));
                        }
                        
                        let mut tc = mem_core::ToolCall {
                            name: tool_name.clone(),
                            arguments: task.tool_args.clone().unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                            id: task.id.to_string(),
                        };
                        for mw in &mws {
                            if let Err(e) = mw.before_tool_call(&mut tc).await {
                                return crate::execution::executor::ExecutionResult {
                                    task_id: task.id,
                                    output: serde_json::Value::String(format!("Safety Block: {}", e)),
                                    success: false,
                                };
                            }
                        }

                        if let Err(e) = security.validate_tool_call(&tool_name) {
                            output = serde_json::Value::String(format!("Security Violation: {}", e));
                        } else {
                            // Robust tool lookup with aliasing and fuzzy matching
                            let mut target_tool = tools.get(&tool_name).await;
                            
                            if target_tool.is_none() {
                                let alias = match tool_name.as_str() {
                                    "duckduckgo_web_search" | "web_search" | "ddg_search" => Some("duckduckgo_search"),
                                    "read_file" => Some("filesystem_read_file"),
                                    _ => None,
                                };
                                if let Some(a) = alias {
                                     target_tool = tools.get(a).await;
                                }
                            }

                            if let Some(tool) = target_tool {
                                match tool.execute(tc.arguments.clone()).await {
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
                        
                        // 2. Learning Gate (Middleware Hook)
                        let mut result_string = output.to_string();
                        for mw in &mws {
                            let _ = mw.after_tool_call(&tc, &mut result_string).await;
                        }
                        
                        if let Some(ref tx) = tx_deep {
                            let _ = tx.send(RuntimeEvent::ToolFinished(tool_name, result_string, success));
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
            
            // 4. INTEGRATE RESULTS INTO CONTEXT
            // We do this before Critique and Adapt to ensure the 7-layers see the latest reality
            for res in results.values() {
                ctx.items.push(mem_core::MemoryItem {
                    role: mem_core::MemoryRole::Tool,
                    content: format!("Tool result: {}", res.output),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    metadata: serde_json::json!({ "task_id": res.task_id }),
                });
            }
            
            // 5. STORE MEMORY (Long-term)
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
            
            tracing::info!(step, feedback = feedback.critique, score = feedback.score, "Phase: ADAPT");
            
            // Completion criteria: Perfect score OR high score without retry suggestion
            if feedback.score >= 1.0 {
                tracing::info!("Goal achieved with perfect score. Terminating loop.");
                completed = true;
            } else if feedback.score > 0.9 && !feedback.suggests_retry {
                tracing::info!("Goal achieved with high score and no retry recommendation.");
                completed = true;
            } else if feedback.suggests_retry || feedback.score <= 0.9 {
                critique = Some(feedback.critique.clone());
                tracing::info!("Re-plan scheduled for next iteration due to critic feedback or low score");
            }
        }

        // 6. FINAL SUMMARY (Conversational)
        if let Some(ref tx) = tx {
            let _ = tx.send(RuntimeEvent::Status("Finalizing report...".into()));
        }

        // Deduplication: If the planner already provided a substantial answer (not boilerplate)
        // and it was already sent to the user as a TextChunk, we can skip the final summary.
        let is_boilerplate = last_plan_content == mem_planner::PLAN_BOILERPLATE_TASKS || last_plan_content == mem_planner::PLAN_BOILERPLATE_NO_TASKS;
        if !last_plan_content.is_empty() && !is_boilerplate && last_plan_content.len() > 150 {
            tracing::info!("Skipping final summary as plan content already provides a substantial answer.");
            if let Some(ref tx) = tx {
                let _ = tx.send(RuntimeEvent::Status("Goal completed.".into()));
            }
            return Ok(last_plan_content);
        }

        let summary_prompt = format!(
            "The user asked: '{}'. \nBased on the steps we have taken and the information gathered in the context, provide a direct, helpful, and natural language response to the user's goal. DO NOT use JSON. Simply answer the question.",
            goal
        );

        let req = crate::llm::LlmRequest {
            prompt: summary_prompt,
            context: Arc::new(ctx.clone()),
            tools: vec![],
        };

        let response = self.llm.generate(req).await?;
        
        if !response.content.is_empty() {
             // Avoid exact duplicates
             if response.content.trim() != last_plan_content.trim() {
                 if let Some(ref tx) = tx {
                    let _ = tx.send(RuntimeEvent::TextChunk(response.content.clone()));
                }
             }
        }

        send_metrics(step, "COMPLETED", &tx, total_input_tokens, total_output_tokens, &ctx);
        if let Some(ref tx) = tx {
            let _ = tx.send(RuntimeEvent::Status("Goal completed.".into()));
        }

        Ok(response.content)
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
                        } else {
                            let mut target_tool = tools.get(&tool_name).await;
                            
                            if target_tool.is_none() {
                                let alias = match tool_name.as_str() {
                                    "duckduckgo_web_search" | "web_search" | "ddg_search" => Some("duckduckgo_search"),
                                    "read_file" => Some("filesystem_read_file"),
                                    _ => None,
                                };
                                if let Some(a) = alias {
                                     target_tool = tools.get(a).await;
                                }
                            }

                            if let Some(tool) = target_tool {
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

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        for mw in &self.middlewares {
            mw.shutdown().await?;
        }
        Ok(())
    }
}
