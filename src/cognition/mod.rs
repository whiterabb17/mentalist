use async_trait::async_trait;
use mem_core::Context;
use mem_planner::{ExecutionPlan, PlannerEngine};
use crate::execution::executor::ExecutionResult;
use std::collections::HashMap;

#[async_trait]
pub trait Planner: Send + Sync {
    async fn create_plan(&self, goal: &str, context: &Context, todo: Option<&str>) -> anyhow::Result<ExecutionPlan>;
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
    async fn create_plan(&self, goal: &str, context: &Context, todo: Option<&str>) -> anyhow::Result<ExecutionPlan> {
        self.engine.plan(goal, context, todo).await
    }
}

pub struct Feedback {
    pub score: f32,
    pub critique: String,
    pub suggests_retry: bool,
}

#[async_trait]
pub trait Critic: Send + Sync {
    async fn evaluate(&self, results: &HashMap<mem_planner::TaskId, ExecutionResult>) -> anyhow::Result<Feedback>;
}

pub struct DefaultCritic;

#[async_trait]
impl Critic for DefaultCritic {
    async fn evaluate(&self, _results: &HashMap<mem_planner::TaskId, ExecutionResult>) -> anyhow::Result<Feedback> {
        Ok(Feedback {
            score: 1.0,
            critique: "Success".into(),
            suggests_retry: false,
        })
    }
}
