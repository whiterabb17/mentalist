#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    FileRead,
    FileWrite,
    Network,
    ShellRestricted,
}

pub struct Policy {
    pub allowed_capabilities: Vec<Capability>,
    pub tool_allowlist: Vec<String>,
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
            .to_string()
    }

    pub fn validate_tool_call(&self, tool_name: &str) -> anyhow::Result<()> {
        if !self.policy.tool_allowlist.contains(&tool_name.to_string()) {
            anyhow::bail!("Security: Tool '{}' is not in the allowlist.", tool_name);
        }
        Ok(())
    }
}
