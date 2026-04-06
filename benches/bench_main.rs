use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mentalist::{Context, Harness, Request, Response, ModelProvider, ResponseChunk};
use mentalist::middleware::Middleware;
use mentalist::executor::{SandboxedExecutor, ExecutionMode, ToolExecutor};
use async_trait::async_trait;
use std::sync::Arc;
use futures_util::stream::{self, BoxStream};
use tokio::runtime::Runtime;

struct NoOpModel;
#[async_trait]
impl mem_core::LlmClient for NoOpModel {
    async fn completion(&self, _prompt: &str) -> anyhow::Result<String> {
        Ok("Bench Completion".into())
    }
}

#[async_trait]
impl ModelProvider for NoOpModel {
    async fn complete(&self, _req: Request) -> anyhow::Result<Response> {
        Ok(Response { content: "ok".into(), tool_calls: vec![] })
    }
    async fn stream_complete(&self, _req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        Ok(Box::pin(stream::empty()))
    }
}
struct NoOpMiddleware;
#[async_trait]
impl Middleware for NoOpMiddleware {
    fn name(&self) -> &str { "NoOpMiddleware" }
}

fn bench_context_cloning(c: &mut Criterion) {
    let mut ctx = Context { items: vec![] };
    for i in 0..100 {
        ctx.items.push(mem_core::MemoryItem {
            role: mem_core::MemoryRole::User,
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

fn bench_middleware_overhead(c: &mut Criterion) {
    let rt = Runtime::new().expect("Benchmark: Failed to create Tokio runtime");
    let provider = Arc::new(NoOpModel);
    let mut harness = Harness::new(provider);
    
    for _ in 0..10 {
        harness.add_middleware(Arc::new(NoOpMiddleware));
    }

    let ctx = Arc::new(Context { items: vec![] });
    let req = Request {
        prompt: "test".into(),
        context: ctx,
        tools: vec![],
    };

    c.bench_function("harness_run_10_middlewares", |b| {
        b.to_async(&rt).iter(|| {
            // Note: req.clone() is still required for the Harness::run signature,
            // but the clones are cheap Arc clones of the context.
            harness.run(black_box(req.clone()))
        })
    });
}

fn bench_executor_efficiency(c: &mut Criterion) {
    let rt = Runtime::new().expect("Benchmark: Failed to create Tokio runtime");
    let temp_dir = std::env::temp_dir().join("mentalist_bench");
    let _ = std::fs::create_dir_all(&temp_dir);
    
    let executor = SandboxedExecutor::new(
        ExecutionMode::Local,
        temp_dir.clone(),
        None
    ).expect("Benchmark: Failed to initialize executor");

    c.bench_function("executor_local_echo", |b| {
        b.to_async(&rt).iter(|| {
            executor.execute(black_box("echo"), black_box(serde_json::json!({ "msg": "hello" })))
        })
    });
}

fn bench_docker_executor(c: &mut Criterion) {
    c.bench_function("bench_docker_executor_stub", |b| b.iter(|| black_box(0)));
}

fn bench_wasm_executor(c: &mut Criterion) {
    c.bench_function("bench_wasm_executor_stub", |b| b.iter(|| black_box(0)));
}

fn bench_json_parsing(c: &mut Criterion) {
    c.bench_function("bench_json_parsing_stub", |b| b.iter(|| {
        let text = r#"{"name": "test", "value": 123, "nested": {"key": "val"}}"#;
        let _ = black_box(serde_json::from_str::<serde_json::Value>(text).expect("Benchmark: Failed to parse JSON"));
    }));
}

fn bench_retry_backoff(c: &mut Criterion) {
    c.bench_function("bench_retry_backoff_stub", |b| b.iter(|| black_box(0)));
}

fn bench_stream_processing(c: &mut Criterion) {
    c.bench_function("bench_stream_processing_stub", |b| b.iter(|| black_box(0)));
}

fn bench_mcp_latency(c: &mut Criterion) {
    c.bench_function("bench_mcp_latency_stub", |b| b.iter(|| black_box(0)));
}

criterion_group!(
    benches,
    bench_context_cloning,
    bench_middleware_overhead,
    bench_executor_efficiency,
    bench_docker_executor,
    bench_wasm_executor,
    bench_json_parsing,
    bench_retry_backoff,
    bench_stream_processing,
    bench_mcp_latency
);
criterion_main!(benches);
