use mentalist::execution::executor::{Executor, ExecutionResult};
use mentalist::execution::graph::TaskGraph;
use mentalist::tools::{ToolRegistry, Tool, ToolSchema};
use mem_planner::{ExecutionPlan, TaskId, TaskNode};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::{sleep, Duration};

struct MockTool;
#[async_trait]
impl Tool for MockTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema { name: "mock".into(), description: "mock".into(), parameters: Value::Null }
    }
    async fn execute(&self, _input: Value) -> anyhow::Result<Value> {
        Ok(serde_json::json!({ "result": "success" }))
    }
}

#[tokio::test]
async fn test_parallel_executor_complex_dag() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(MockTool));
    let executor = Executor::new(Arc::new(tools));

    let mut plan = ExecutionPlan::new();

    // DAG Structure:
    // A -> B
    // A -> C
    // B, C -> D
    // E (Independent)

    let id_a = TaskId::new();
    let id_b = TaskId::new();
    let id_c = TaskId::new();
    let id_d = TaskId::new();
    let id_e = TaskId::new();

    plan.add_task(TaskNode {
        id: id_a.clone(), name: "A".into(), description: "A".into(),
        tool_name: Some("mock".into()), tool_args: None,
        dependencies: vec![], metadata: Value::Null,
    });

    plan.add_task(TaskNode {
        id: id_b.clone(), name: "B".into(), description: "B".into(),
        tool_name: Some("mock".into()), tool_args: None,
        dependencies: vec![id_a.clone()], metadata: Value::Null,
    });

    plan.add_task(TaskNode {
        id: id_c.clone(), name: "C".into(), description: "C".into(),
        tool_name: Some("mock".into()), tool_args: None,
        dependencies: vec![id_a.clone()], metadata: Value::Null,
    });

    plan.add_task(TaskNode {
        id: id_d.clone(), name: "D".into(), description: "D".into(),
        tool_name: Some("mock".into()), tool_args: None,
        dependencies: vec![id_b.clone(), id_c.clone()], metadata: Value::Null,
    });

    plan.add_task(TaskNode {
        id: id_e.clone(), name: "E".into(), description: "E".into(),
        tool_name: Some("mock".into()), tool_args: None,
        dependencies: vec![], metadata: Value::Null,
    });

    let graph = TaskGraph::new(plan);
    let counter = Arc::clone(&call_count);
    
    let results = executor.execute_parallel(&graph, move |task| {
        let c = Arc::clone(&counter);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(10)).await;
            ExecutionResult {
                task_id: task.id,
                output: serde_json::json!({ "status": "ok" }),
                success: true,
            }
        }
    }).await.unwrap();

    assert_eq!(results.len(), 5);
    assert_eq!(call_count.load(Ordering::SeqCst), 5, "All 5 tasks in the DAG should have been executed");
    
    // Ensure all tasks succeeded
    for id in &[id_a, id_b, id_c, id_d, id_e] {
        assert!(results.get(id).unwrap().success);
    }
}
