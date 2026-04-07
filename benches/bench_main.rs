use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mentalist::{AgentRuntime, ExecutionLimits};
use mentalist::cognition::{Planner, Critic, Feedback};
use mentalist::execution::executor::{Executor, ExecutionResult};
use mentalist::execution::graph::TaskGraph;
use mentalist::memory::{MemoryStore, MemoryEvent, MemoryQuery};
use mentalist::llm::LLMProvider;
use mentalist::tools::{Tool, ToolRegistry, ToolSchema};
use mentalist::security::{SecurityEngine, Policy};
use mem_core::{Context, MemoryItem, MemoryRole, Request, Response, ResponseChunk};
use mem_planner::{ExecutionPlan, TaskId, TaskNode};
use async_trait::async_trait;
use std::sync::Arc;
use futures_util::stream::{self, BoxStream};
use tokio::runtime::Runtime;
use std::collections::HashMap;

// --- Mocks for Benchmarking ---

struct NoOpModel;
#[async_trait]
impl mem_core::LlmClient for NoOpModel {
    async fn completion(&self, _prompt: &str) -> anyhow::Result<String> {
        Ok("Bench Completion".into())
    }
}

#[async_trait]
impl mem_core::ModelProvider for NoOpModel {
    async fn complete(&self, _req: Request) -> anyhow::Result<Response> {
        Ok(Response { content: "ok".into(), tool_calls: vec![] })
    }
    async fn stream_complete(&self, _req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        Ok(Box::pin(stream::empty()))
    }
}

#[async_trait]
impl LLMProvider for NoOpModel {
    async fn generate(&self, _req: Request) -> anyhow::Result<Response> {
        Ok(Response { content: "ok".into(), tool_calls: vec![] })
    }
    async fn generate_stream(&self, _req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        Ok(Box::pin(stream::empty()))
    }
}

struct MockPlanner;
#[async_trait]
impl Planner for MockPlanner {
    async fn create_plan(&self, _goal: &str, _context: &Context, _todo: Option<&str>) -> anyhow::Result<ExecutionPlan> {
        let mut plan = ExecutionPlan::new();
        let task_id = TaskId("task-1".into());
        plan.add_task(TaskNode {
            id: task_id,
            name: "test task".into(),
            description: "test description".into(),
            tool_name: Some("test_tool".into()),
            tool_args: Some(serde_json::json!({})),
            dependencies: vec![],
            metadata: serde_json::json!({}),
        });
        Ok(plan)
    }
}

struct MockCritic;
#[async_trait]
impl Critic for MockCritic {
    async fn evaluate(&self, _results: &HashMap<TaskId, ExecutionResult>) -> anyhow::Result<Feedback> {
        Ok(Feedback { score: 1.0, critique: "Good".into(), suggests_retry: false })
    }
}

struct MockMemory;
#[async_trait]
impl MemoryStore for MockMemory {
    async fn store(&self, _event: MemoryEvent) -> anyhow::Result<()> { Ok(()) }
    async fn recall(&self, _query: MemoryQuery) -> anyhow::Result<Vec<MemoryEvent>> { Ok(vec![]) }
    async fn summarize(&self, _ctx: &mut Context) -> anyhow::Result<String> { Ok("Summary".into()) }
}

struct TestTool;
#[async_trait]
impl Tool for TestTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "test_tool".into(),
            description: "test tool description".into(),
            parameters: serde_json::json!({}),
            source: "builtin".into(),
        }
    }
    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({"status": "ok"}))
    }
}

// --- Benchmarks ---

fn bench_context_cloning(c: &mut Criterion) {
    let mut ctx = Context { items: vec![] };
    for i in 0..100 {
        ctx.items.push(MemoryItem {
            role: MemoryRole::User,
            content: format!("Message {}", i),
            timestamp: 0,
            metadata: serde_json::json!({}),
        });
    }
    let arc_ctx = Arc::new(ctx);

    c.bench_function("context_cloning_100_items", |b| {
        b.iter(|| {
            let _ = black_box((*arc_ctx).clone());
        })
    });
}

fn bench_executor_efficiency(c: &mut Criterion) {
    let rt = Runtime::new().expect("Benchmark: Failed to create Tokio runtime");
    let executor = rt.block_on(async {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(TestTool)).await;
        Arc::new(Executor::new(Arc::new(registry)))
    });
    
    let mut plan = ExecutionPlan::new();
    let task_id = TaskId("task-1".into());
    plan.add_task(TaskNode {
        id: task_id.clone(),
        name: "test task".into(),
        description: "test description".into(),
        tool_name: Some("test_tool".into()),
        tool_args: Some(serde_json::json!({})),
        dependencies: vec![],
        metadata: serde_json::json!({}),
    });
    let graph = TaskGraph::new(plan);

    c.bench_function("executor_parallel_1_task", |b| {
        b.to_async(&rt).iter(|| {
            let executor = Arc::clone(&executor);
            let graph = graph.clone();
            let task_id_inner = task_id.clone();
            async move {
                executor.execute_parallel(&graph, move |task| {
                    let _tid = task_id_inner.clone();
                    // Fix: task is TaskNode, so we can use its ID
                    let tid_from_task = task.id.clone();
                    async move {
                        ExecutionResult {
                            task_id: tid_from_task,
                            output: serde_json::json!({"status": "ok"}),
                            success: true,
                        }
                    }
                }).await
            }
        })
    });
}

fn bench_runtime_overhead(c: &mut Criterion) {
    let rt = Runtime::new().expect("Benchmark: Failed to create Tokio runtime");
    
    let tool_registry = rt.block_on(async {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(TestTool)).await;
        Arc::new(registry)
    });
    
    let policy = Policy {
        allowed_capabilities: vec![],
        tool_allowlist: vec!["test_tool".into()],
    };
    
    let runtime = AgentRuntime {
        planner: Arc::new(MockPlanner),
        executor: Arc::new(Executor::new(Arc::clone(&tool_registry))),
        memory: Arc::new(MockMemory),
        llm: Arc::new(NoOpModel),
        tools: tool_registry,
        security: Arc::new(SecurityEngine::new(policy)),
        critic: Arc::new(MockCritic),
        limits: ExecutionLimits { max_steps: 1, timeout_seconds: 10 },
    };

    let ctx = Context { items: vec![] };

    c.bench_function("agent_runtime_1_step", |b| {
        b.to_async(&rt).iter(|| {
            runtime.run("test goal", ctx.clone(), None)
        })
    });
}

fn bench_json_parsing(c: &mut Criterion) {
    c.bench_function("bench_json_parsing", |b| b.iter(|| {
        let text = r#"{"name": "test", "value": 123, "nested": {"key": "val"}}"#;
        let _ = black_box(serde_json::from_str::<serde_json::Value>(text).expect("Benchmark: Failed to parse JSON"));
    }));
}

criterion_group!(
    benches,
    bench_context_cloning,
    bench_executor_efficiency,
    bench_runtime_overhead,
    bench_json_parsing
);
criterion_main!(benches);


