# Middleware Development Guide

Middlewares are the heart of the Mentalist agentic loop. They allow you to intercept, modify, and enhance the interaction between the AI model and its tools.

## Lifecycle Hooks

The `Middleware` trait defines several hooks that are called at different stages of the execution:

1. **`before_ai_call`**: Called before the request is sent to the LLM. Use this for context optimization, prompt engineering, or checking safety constraints.
2. **`after_ai_call`**: Called after the LLM returns its response but before it is processed. Great for parsing custom formats or intent extraction.
3. **`before_tool_call`**: Called before a tool is executed. This is where you can implement security policies (e.g., blocking certain file paths) or asking for human-in-the-loop approval.
4. **`after_tool_call`**: Called after a tool completes. Use this to post-process tool output, log results, or handle common tool errors.
5. **`optimize_context`**: A manual hook for summarizing long threads or pruning irrelevant history.

## Creating a Simple Middleware

To create a middleware, implement the `Middleware` trait:

```rust
use async_trait::async_trait;
use mentalist::middleware::Middleware;
use mentalist::{Request, Response, ToolCall};

pub struct MyLoggingMiddleware;

#[async_trait]
impl Middleware for MyLoggingMiddleware {
    fn name(&self) -> &str { "LoggingMiddleware" }

    async fn before_ai_call(&self, req: &mut Request) -> anyhow::Result<()> {
        println!("Prompt: {}", req.prompt);
        Ok(())
    }

    async fn after_tool_call(&self, tool: &ToolCall, result: &mut String) -> anyhow::Result<()> {
        println!("Tool {} result: {}", tool.name, result);
        Ok(())
    }
}
```

## Adding Middleware to the Harness

Once you've implemented your middleware, add it to the `Harness`:

```rust
use std::sync::Arc;
let mut harness = Harness::new(provider);
harness.add_middleware(Arc::new(MyLoggingMiddleware));
```

## Best Practices

*   **Be Fast**: Middlewares are executed sequentially. Avoid long-running operations.
*   **Error Handling**: If a middleware returns `Err`, the entire execution chain is halted (unless it's during streaming, where we log and continue).
*   **Side Effects**: Keep side effects (like database writes) to a minimum or wrap them in safe abstractions.
*   **Context Safety**: When modifying the `Request::context`, be careful not to remove essential system instructions.
