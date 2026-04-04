# Security Model

The Mentalist security perimeter takes defense-in-depth extremely seriously. Because LLMs run untrusted strings generated dynamically, our entire `SandboxedExecutor` and `SkillExecutor` components are built to mitigate local traversal or malicious execution paths.

## 1. Tool Sandboxing (`SandboxedExecutor`)
Rather than piping arbitrary queries out to `bash -c`, Mentalist defaults to parameterized process spawning isolated from shell metacharacters.

### Command Whitelist Validation
Every tool request maps against a stringent `CommandValidator`. 
> [!IMPORTANT]  
> If an operator forces `ExecutionMode::Local`, they are confined to 15 explicit whitelisted binaries (`python`, `cat`, `echo`, `grep`, `jq`, `node`, `curl`, `wget`, `zip`, `bash`, `sh`, `ls`, `tar`, `find`, `ruby`).

### Shell Injection Defences
We validate every argument using the `;&|`$()[]{}"'\` metacharacter blocklist.
No relative `../` or absolute `/` arguments are permitted by default unless executed explicitly via safe file bounds in a Vault.

### Execution Tiers
Users scale isolation dynamically:
1. **Network (Docker)**
   - Provisions ephemeral sandboxes dynamically via `bollard`. 
   - Strict defaults: **4GB max memory** and **50,000 CPU Quota** (50%).
   - Mount paths strictly enforce `/sandbox` isolation.

2. **WASM (Highest Security)**
   - Executions are handled natively within the memory space using Wasmtime v36.
   - Resource bound limits: **4GB Memory**, **1MB Sandbox Stack**, **50 Million Instructions Fueling bounds** per trigger.

## 2. Directory Path Validations (`SkillExecutor`)
Skills loaded via the filesystem (`SKILL.md`) natively generate code runs.
- **Escape Checking:** Skill paths call Rust's `canonicalize()` standard against the project bound roots before evaluating scripts. 
- **Symlink Prevention:** Symlinked `.sh` or `.py` hooks are ignored dynamically to mitigate out-of-boundary malicious references.
- **Run Triggers:** Timeout limiters forcibly suspend execution after **30s block timers**.

## 3. Environment Sanitizations
Process trees explicitly block hostile path environment injections. 
Only vital OS components are persisted dynamically into run nodes: `PATH`, `SYSTEMROOT`, `SYSTEMDRIVE`, `TEMP`, `TMP`, and `USERPROFILE`.
