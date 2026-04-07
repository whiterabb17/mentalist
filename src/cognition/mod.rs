use async_trait::async_trait;
use mem_core::Context;
use mem_planner::{ExecutionPlan, PlannerEngine};
use crate::execution::executor::ExecutionResult;
use std::collections::HashMap;

#[async_trait]
pub trait Planner: Send + Sync {
    async fn create_plan(&self, goal: &str, context: &Context, tools: Vec<crate::tools::ToolSchema>, todo: Option<&str>) -> anyhow::Result<ExecutionPlan>;
}

pub struct MindPalacePlanner {
    pub engine: std::sync::Arc<dyn PlannerEngine>,
}

impl MindPalacePlanner {
    pub fn new(engine: std::sync::Arc<dyn PlannerEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl Planner for MindPalacePlanner {
    async fn create_plan(&self, goal: &str, context: &Context, tools: Vec<crate::tools::ToolSchema>, todo: Option<&str>) -> anyhow::Result<ExecutionPlan> {
        let definitions = tools.into_iter().map(|s| mem_core::ToolDefinition {
            name: s.name,
            description: s.description,
            parameters: s.parameters,
        }).collect();
        self.engine.plan(goal, context, definitions, todo).await
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Feedback {
    pub score: f32,
    pub critique: String,
    pub suggests_retry: bool,
}

#[async_trait]
pub trait Critic: Send + Sync {
    async fn evaluate(&self, goal: &str, context: &Context, results: &HashMap<mem_planner::TaskId, ExecutionResult>) -> anyhow::Result<Feedback>;
}

pub struct DefaultCritic;

#[async_trait]
impl Critic for DefaultCritic {
    async fn evaluate(&self, _goal: &str, _context: &Context, _results: &HashMap<mem_planner::TaskId, ExecutionResult>) -> anyhow::Result<Feedback> {
        Ok(Feedback {
            score: 1.0,
            critique: "Success".into(),
            suggests_retry: false,
        })
    }
}

pub struct LlmCritic {
    pub provider: std::sync::Arc<dyn crate::llm::LLMProvider>,
}

impl LlmCritic {
    pub fn new(provider: std::sync::Arc<dyn crate::llm::LLMProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl Critic for LlmCritic {
    async fn evaluate(&self, goal: &str, context: &Context, results: &HashMap<mem_planner::TaskId, ExecutionResult>) -> anyhow::Result<Feedback> {
        let results_json = serde_json::to_string_pretty(results)?;
        let context_json = serde_json::to_string_pretty(context)?;

        let prompt = format!(
            r#"You are the Critic Module of a Cognitive Agent.
Your task is to evaluate if the high-level goal has been achieved based on the execution results of the planned tasks.

### GOAL ###
{}

### CONTEXT ###
{}

### EXECUTION RESULTS ###
{}

### INSTRUCTIONS ###
1. Assess if the goal is fully met.
2. Provide a score between 0.0 and 1.0 (1.0 = Perfect, 0.0 = Failed).
3. If not perfect, provide a critique and set suggests_retry to true.
4. Output your response as JSON.

JSON OUTPUT:
"#,
            goal, context_json, results_json
        );

        let req = crate::llm::LlmRequest {
            prompt,
            context: std::sync::Arc::new(context.clone()),
            tools: vec![],
        };

        let response = self.provider.generate(req).await?;
        let content = response.content;

        let json_str = if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                &content[start..=end]
            } else {
                &content[start..]
            }
        } else {
            &content
        };

        let feedback: Feedback = serde_json::from_str(json_str).unwrap_or(Feedback {
            score: 0.0,
            critique: format!("Failed to parse critic response: {}", content),
            suggests_retry: true,
        });

        Ok(feedback)
    }
}

pub enum RuntimeEvent {
    MetricUpdate {
        step: usize,
        phase: String,
        input_tokens: usize,
        output_tokens: usize,
        context_size: usize,
    },
    AwaitingApproval(mem_planner::ExecutionPlan),
}
