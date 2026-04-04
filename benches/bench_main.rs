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
    let rt = Runtime::new().unwrap();
    let provider = Box::new(NoOpModel);
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
            harness.run(black_box(req.clone()))
        })
    });
}

fn bench_executor_efficiency(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let temp_dir = std::env::temp_dir().join("mentalist_bench");
    let _ = std::fs::create_dir_all(&temp_dir);
    
    let executor = SandboxedExecutor::new(
        ExecutionMode::Local,
        temp_dir.clone(),
        None
    ).unwrap();

    c.bench_function("executor_local_echo", |b| {
        b.to_async(&rt).iter(|| {
            executor.execute(black_box("echo"), black_box(serde_json::json!({ "msg": "hello" })))
        })
    });
}

criterion_group!(benches, bench_context_cloning, bench_middleware_overhead, bench_executor_efficiency);
criterion_main!(benches);
