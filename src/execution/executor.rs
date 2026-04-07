use std::sync::Arc;
use crate::tools::ToolRegistry;
use crate::execution::graph::TaskGraph;
use mem_planner::{TaskId, TaskNode};
use std::collections::{HashSet, HashMap};

pub struct ExecutionResult {
    pub task_id: TaskId,
    pub output: serde_json::Value,
    pub success: bool,
}

pub struct Executor {
    pub tools: Arc<ToolRegistry>,
}

impl Executor {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self { tools }
    }

    pub async fn execute_parallel<F, Fut>(
        &self,
        graph: &TaskGraph,
        task_fn: F,
    ) -> anyhow::Result<HashMap<TaskId, ExecutionResult>>
    where
        F: Fn(TaskNode) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ExecutionResult> + Send,
    {
        let mut completed = HashSet::new();
        let mut results = HashMap::new();
        let task_fn = Arc::new(task_fn);

        while !graph.is_complete(&completed) {
            let ready = graph.get_ready_tasks(&completed);
            if ready.is_empty() {
                anyhow::bail!("Deadlock detected in TaskGraph (no ready tasks)");
            }

            let mut set = tokio::task::JoinSet::new();
            for task in ready {
                let f = Arc::clone(&task_fn);
                set.spawn(async move { f(task).await });
            }

            while let Some(res) = set.join_next().await {
                let result = res.map_err(|e| anyhow::anyhow!("Task join error: {}", e))?;
                completed.insert(result.task_id.clone());
                results.insert(result.task_id.clone(), result);
            }
        }

        Ok(results)
    }
}
