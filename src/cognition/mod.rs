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
        let plan = self.engine.plan(goal, context, definitions, todo).await?;

        Ok(plan)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Feedback {
    pub score: f32,
    #[serde(alias = "explanation")]
    pub critique: String,
    #[serde(alias = "suggested_retry")]
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
Your task is to evaluate if the high-level user goal has been fully achieved based on the execution results of the planned tasks.

### GOAL ###
{}

### CONTEXT ###
{}

### EXECUTION RESULTS ###
{}

### INSTRUCTIONS ###
1. Assess if the user's explicit goal is fully met.
2. DECISIVENESS: If the goal is to "research" or "find" something, and the results are present in the context, the goal is ACHIEVED. DO NOT suggest a retry just for "more details" unless the user explicitly asked for them.
3. NO PEDANTRY: If the user asked for a list and you have a list, the goal is achieved.
4. If most tasks succeeded and the core objective is met, give a high score (0.95-1.0).
5. Output your response as a SINGLE JSON OBJECT matching the schema below.

REQUIRED SCHEMA:
{{
  "score": float,
  "critique": "string",
  "suggests_retry": boolean
}}

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

        // Aggressive parsing to handle LLM variations
        let feedback: Feedback = if let Ok(f) = serde_json::from_str::<Feedback>(json_str) {
            f
        } else {
            // Check if LLM wrapped feedback in task IDs (e.g. {"task_1": { "score": 0, ... }})
            let map: Result<HashMap<String, Feedback>, _> = serde_json::from_str(json_str);
            if let Ok(m) = map {
                // Return the first failing task as representative feedback, or a summary
                m.into_values().next().unwrap_or(Feedback {
                    score: 0.0,
                    critique: "Empty feedback map received".into(),
                    suggests_retry: true,
                })
            } else {
                Feedback {
                    score: 0.0,
                    critique: format!("Failed to parse critic response: {}", content),
                    suggests_retry: true,
                }
            }
        };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feedback_parsing_aliases() {
        let json = r#"{
            "score": 0.5,
            "explanation": "Needs more detail",
            "suggested_retry": true
        }"#;
        let feedback: Feedback = serde_json::from_str(json).unwrap();
        assert_eq!(feedback.score, 0.5);
        assert_eq!(feedback.critique, "Needs more detail");
        assert!(feedback.suggests_retry);
    }

    #[test]
    fn test_feedback_parsing_standard() {
        let json = r#"{
            "score": 1.0,
            "critique": "Perfect",
            "suggests_retry": false
        }"#;
        let feedback: Feedback = serde_json::from_str(json).unwrap();
        assert_eq!(feedback.score, 1.0);
        assert_eq!(feedback.critique, "Perfect");
        assert!(!feedback.suggests_retry);
    }
}
