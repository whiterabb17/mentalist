# Model Context Protocol (MCP) Integration

Mentalist supports robust standardized communication securely matching bounds natively via the standard [Model Context Protocol](https://modelcontextprotocol.io).

## McpExecutor API
The integration uses a decoupled process tree spawned recursively mapping out a JSON-RPC 2.0 streaming parser directly matching the standard out arrays matching native constraints securely.

### Serialized Execution Lines
Subprocesses heavily execute async requests asynchronously mapping over a synchronized constraint lock consistently.
The structure holds an `Arc<Mutex<()>>` explicit bound `call_lock` preventing interleaved standard outputs from generating malformed JSON arrays efficiently ensuring correct serialization constraints.

### Response Parsing
- **Dangerous Unwraps Eliminated:** Resolves against standard JSON-RPC specifications mapping `result` and `error` parameters defensively, extracting specific internal errors safely bounding native MCP formats successfully (Addresses Issue #4).
- **Timeouts:** Pushes a strict 10s timeout bound using `tokio::time::timeout` natively limiting hanging `initialize` states robustly dynamically terminating orphaned blocks.
- **Content Aggregation:** Parses arrays of structures flattening output nodes securely resolving strings via native JSON path arrays efficiently.

## Built-in Servers

We provide ergonomic wrappers using the latest MCP ecosystems natively mapping `npx` endpoints efficiently dynamically (automatically switching to `npx.cmd` structurally extending bindings securely safely against Windows machines):
```rust
use mentalist::mcp::BuiltinMcp;

// Access local mapped folders
let files = BuiltinMcp::filesystem(vec!["/my/local/folder".to_string()]);

// Map Firecrawl mapping extraction vectors correctly
let fire = BuiltinMcp::firecrawl("fc-api-key".to_string());
```

> [!TIP]
> Use `MultiExecutor` arrays parsing vectors containing elements combining arrays mapping `McpExecutor` arrays against standard native bounds gracefully mapping global pipelines natively.
