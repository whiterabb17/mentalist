# Performance Tuning

Mentalist runs robust language pipelines designed to stay highly available without compromising thread locking or consuming disproportionate memory bandwidth dynamically.

## 1. Copy-On-Write Contexts (Zero Cost Appends)
The biggest overhead observed in traditional LLM frameworks occurs when duplicating context memory windows arrays dynamically during turns.
Mentalist utilizes contiguous `Arc::make_mut` bounds wrapped recursively around the `Context` arrays in `src/agent.rs`.
> [!TIP]
> Rather than locking the conversational memory with heavy `tokio::sync::Mutex` paths, `DeepAgentState` triggers a shallow write over thread boundaries, resulting in nanosecond-level array allocations.

## 2. Streaming Decoupling
Because `DeepAgent::step_stream` handles text synchronously parsing tools alongside standard string generation, the LLM driver is never hung on polling blocks.
- Tool responses format explicitly to JSON backbones automatically. 
- Stream cycles use an iterative parsing accumulator `current_tool_args` ensuring real-time response times bounding.

## 3. Transient Backoffs
`Mentalist` uses exponentially growing fallback mechanics without polluting thread resources:
```rust
// Exponential Tool Executions
let backoff = 2u64.pow(retry_count as u32);
tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
```
If a tool encounters a transient timeout boundary, thread loops fall back linearly giving the container layers time to gracefully recover rather than panicking instances globally.

## 4. MCP IO Caching
When invoking Model Protocol Servers via `mcp.rs`, the executable avoids constantly spawning binaries dynamically. 
Pipelines lock cleanly using generic asynchronous mutex bindings `tokio::sync::Mutex`. JSON-RPC responses are extracted eagerly reading memory pools from standard standard out, dropping terminal noisy messages out of scope.
