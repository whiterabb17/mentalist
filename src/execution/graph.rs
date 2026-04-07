use mem_planner::{ExecutionPlan, TaskId, TaskNode};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct TaskGraph {
    pub plan: ExecutionPlan,
}

impl TaskGraph {
    pub fn new(plan: ExecutionPlan) -> Self {
        Self { plan }
    }

    /// Resolves the tasks that are ready to execute (all dependencies completed).
    pub fn get_ready_tasks(&self, completed: &HashSet<TaskId>) -> Vec<TaskNode> {
        self.plan.tasks.values()
            .filter(|t| !completed.contains(&t.id))
            .filter(|t| t.dependencies.iter().all(|d| completed.contains(d)))
            .cloned()
            .collect()
    }

    pub fn is_complete(&self, completed: &HashSet<TaskId>) -> bool {
        self.plan.tasks.is_empty() || self.plan.tasks.values().all(|t| completed.contains(&t.id))
    }
}
