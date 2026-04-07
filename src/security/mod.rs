#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Capability {
    #[default]
    FileRead,
    FileWrite,
    Network,
    ShellRestricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Policy {
    pub allowed_capabilities: Vec<Capability>,
    pub tool_allowlist: Vec<String>,
}

impl Policy {
    pub fn new(allowed_capabilities: Vec<Capability>, tool_allowlist: Vec<String>) -> Self {
        Self {
            allowed_capabilities,
            tool_allowlist,
        }
    }
}

pub struct SecurityEngine {
    pub policy: Policy,
}

impl SecurityEngine {
    pub fn new(policy: Policy) -> Self {
        Self { policy }
    }

    pub fn sanitize_prompt(&self, prompt: &str) -> String {
        prompt
            .replace("ignore previous instructions", "[REDACTED]")
            .replace("system override", "[REDACTED]")
    }

    pub fn validate_tool_call(&self, tool_name: &str) -> anyhow::Result<()> {
        if !self.policy.tool_allowlist.is_empty() && !self.policy.tool_allowlist.contains(&tool_name.to_string()) {
            anyhow::bail!("Security: Tool '{}' is not in the allowlist.", tool_name);
        }
        Ok(())
    }
}
