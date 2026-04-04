# Executor Modes Guide

The `SandboxedExecutor` maps isolated bounds over execution traces using the robust polymorphic enum `ExecutionMode`. Each executor controls its own system context dependencies natively bounding against a dedicated Vault space securely.

## 1. Local Mode (`ExecutionMode::Local`)
Runs directly on the host machine using standard command mapping algorithms resolving local binaries.
- **Fallbacks:** Automatically matches commands portably across distributions natively (e.g. `python` falls back sequentially evaluating `python3`). Matches `node -> nodejs` and `pip -> pip3`.
- **Environment Handling:** To guarantee isolated bounds consistently, Local clears `env_clear()` aggressively leaving only `PATH`, `SYSTEMROOT`, `SYSTEMDRIVE`, `TEMP`, `TMP`, and `USERPROFILE`.
> [!CAUTION]
> Local mode provides **no container isolation**. It relies strictly on `CommandValidator` and assumes binaries are trusted. Only use this mode within controlled deployments.

## 2. Docker Mode (`ExecutionMode::Docker`)
Provisions dynamic container layers resolving requests safely using the `bollard` v0.20 ecosystem dynamically bounds.
- **Resource Limits:** Mapped structurally injecting configurable `memory_limit` (defaults strictly to 4GB limits) and a `cpu_quota` mapping limiting cores consistently (default 50,000 / 50%).
- **Mount Binding:** Maps current vault limits directly into the core space mapping against `/sandbox` explicitly bounding read-write scopes.
- **Image Lifecycle:** Streams Image Pull arrays correctly mapping via native API endpoints, automatically dropping containers persistently on bounds even if errors occur forcibly using the `RemoveContainerOptions` block.

## 3. Wasm Mode (`ExecutionMode::Wasm`)
The most verifiable restricted bounding execution instance mapped using native Wasmtime 36.0.6 APIs safely.
- **WASI Sandboxing:** Embeds `builder.env()` mapping arguments against exact subsets directly blocking implicit injection vectors.
- **Fuel Tracking:** Execution threads forcefully bind `store.set_fuel(50_000_000)` allocating bounds protecting instance infinite loop behaviors consistently.
- **Storage Mapping:** Preopens files using explicitly limited directories via `builder.preopened_dir` against bounds matching strictly against WASI directory descriptors (`preview1` arrays) securely.
- **Memory Arrays:** Uses custom `ResourceLimiter` blocks allocating 4GB ceilings against page faults arrays efficiently ensuring bounds stay memory-safe securely natively.
