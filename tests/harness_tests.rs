use mentalist::{Harness, Request, Response, ModelProvider, ToolCall, ResponseChunk, Context};
use async_trait::async_trait;
use futures_util::stream::{self, BoxStream};
use std::sync::Arc;

struct MockModelProvider {
    pub response: String,
}

#[async_trait]
impl mem_core::LlmClient for MockModelProvider {
    async fn completion(&self, _prompt: &str) -> anyhow::Result<String> {
        Ok(self.response.clone())
    }
}

#[async_trait]
impl ModelProvider for MockModelProvider {
    async fn complete(&self, _req: Request) -> anyhow::Result<Response> {
        Ok(Response {
            content: self.response.clone(),
            tool_calls: vec![],
        })
    }

    async fn stream_complete(&self, _req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        let chunk = ResponseChunk {
            content_delta: Some(self.response.clone()),
            tool_call_delta: None,
            usage: None,
            is_final: true,
        };
        Ok(Box::pin(stream::iter(vec![Ok(chunk)])))
    }
}

#[tokio::test]
async fn test_harness_lifecycle_basic() {
    let provider = Arc::new(MockModelProvider { response: "Hello Gypsy".to_string() });
    let harness = Harness::new(provider);
    
    let req = Request {
        prompt: "Hi".to_string(),
        context: Arc::new(Context { items: vec![] }),
        tools: vec![],
    };
    
    let res = harness.run(req).await.unwrap();
    assert_eq!(res.content, "Hello Gypsy");
}

#[tokio::test]
async fn test_harness_middleware_execution() {
    use mentalist::middleware::Middleware;
    
    struct SpyMiddleware {
        pub called: Arc<tokio::sync::Mutex<bool>>,
    }
    
    #[async_trait]
    impl Middleware for SpyMiddleware {
        async fn before_ai_call(&self, _req: &mut Request) -> anyhow::Result<()> {
            let mut called = self.called.lock().await;
            *called = true;
            Ok(())
        }
    }
    
    let called = Arc::new(tokio::sync::Mutex::new(false));
    let provider = Arc::new(MockModelProvider { response: "Ok".to_string() });
    let mut harness = Harness::new(provider);
    harness.add_middleware(Arc::new(SpyMiddleware { called: called.clone() }));
    
    let req = Request {
        prompt: "test".to_string(),
        context: Arc::new(Context { items: vec![] }),
        tools: vec![],
    };
    
    let _ = harness.run(req).await.unwrap();
    
    assert!(*called.lock().await);
}

#[tokio::test]
async fn test_harness_tool_hooks() {
    use mentalist::middleware::Middleware;
    
    struct ToolSpy {
        pub before_called: Arc<tokio::sync::Mutex<bool>>,
        pub after_called: Arc<tokio::sync::Mutex<bool>>,
    }
    
    #[async_trait]
    impl Middleware for ToolSpy {
        async fn before_tool_call(&self, _tool: &mut ToolCall) -> anyhow::Result<()> {
            let mut bc = self.before_called.lock().await;
            *bc = true;
            Ok(())
        }
        async fn after_tool_call(&self, _tool: &ToolCall, _result: &mut String) -> anyhow::Result<()> {
            let mut ac = self.after_called.lock().await;
            *ac = true;
            Ok(())
        }
    }
    
    let bc = Arc::new(tokio::sync::Mutex::new(false));
    let ac = Arc::new(tokio::sync::Mutex::new(false));
    let provider = Arc::new(MockModelProvider { response: "Ok".to_string() });
    let mut harness = Harness::new(provider);
    harness.add_middleware(Arc::new(ToolSpy { 
        before_called: bc.clone(), 
        after_called: ac.clone() 
    }));
    
    let mut tool = ToolCall {
        name: "test_tool".to_string(),
        arguments: serde_json::json!({}),
    };
    
    harness.run_before_tool_hooks(&mut tool).await.unwrap();
    assert!(*bc.lock().await);
    
    let mut result = "Tool Success".to_string();
    harness.run_after_tool_hooks(&tool, &mut result).await.unwrap();
    assert!(*ac.lock().await);
}

#[tokio::test]
async fn test_harness_middleware_naming() {
    use mentalist::middleware::Middleware;
    
    struct NamedMiddleware;
    #[async_trait]
    impl Middleware for NamedMiddleware {
        fn name(&self) -> &str { "CustomSpy" }
        async fn before_ai_call(&self, _req: &mut Request) -> anyhow::Result<()> {
            anyhow::bail!("Intentional Failure")
        }
    }
    
    let provider = Arc::new(MockModelProvider { response: "Ok".to_string() });
    let mut harness = Harness::new(provider);
    harness.add_middleware(Arc::new(NamedMiddleware));
    
    let req = Request {
        prompt: "test".to_string(),
        context: Arc::new(Context { items: vec![] }),
        tools: vec![],
    };
    
    let res = harness.run(req).await;
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    // Verify that the middleware name is captured in the error context
    assert!(err_msg.contains("'CustomSpy'"));
}

#[tokio::test]
async fn test_logging_middleware_invocation() {
    // This mostly verifies it doesn't crash, as tracing is hard to capture in unit tests without specialized subscribers
    use mentalist::middleware::LoggingMiddleware;
    
    let provider = Arc::new(MockModelProvider { response: "Ok".to_string() });
    let mut harness = Harness::new(provider);
    harness.add_middleware(Arc::new(LoggingMiddleware));
    
    let req = Request {
        prompt: "test".to_string(),
        context: Arc::new(Context { items: vec![] }),
        tools: vec![],
    };
    
    let res = harness.run(req).await;
    assert!(res.is_ok());
}
