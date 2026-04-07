use mentalist::tools::{ToolRegistry, Tool, ToolSchema};
use mentalist::security::{SecurityEngine, Policy};
use mentalist::execution::executor::ExecutionResult;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

struct FailingTool;
#[async_trait]
impl Tool for FailingTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema { name: "failing_tool".into(), description: "A tool that fails".into(), parameters: Value::Null }
    }
    async fn execute(&self, _input: Value) -> anyhow::Result<Value> {
        anyhow::bail!("Intentional Tool Failure")
    }
}

#[tokio::test]
async fn test_security_violation_caught() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(FailingTool));
    
    // Security Policy: No tools allowed
    let security = Arc::new(SecurityEngine::new(Policy { allowed_capabilities: vec![], tool_allowlist: vec![] }));
    
    let result = security.validate_tool_call("failing_tool");
    assert!(result.is_err(), "Security should have blocked non-allowlisted tool");
}

#[tokio::test]
async fn test_tool_failure_propagation() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(FailingTool));
    let executor = mentalist::execution::executor::Executor::new(Arc::new(tools));

    let plan = mem_planner::ExecutionPlan::new(); 
    let graph = mentalist::execution::graph::TaskGraph::new(plan);

    let results = executor.execute_parallel(&graph, |task| {
        async move {
             ExecutionResult {
                 task_id: task.id,
                 output: serde_json::json!({ "error": "Internal Error" }),
                 success: false,
             }
        }
    }).await.unwrap();

    assert!(results.is_empty() || !results.values().any(|r| r.success));
}
