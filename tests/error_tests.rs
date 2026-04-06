use mentalist::error::{MentalistError, Result};
use mentalist::executor::ToolError;

#[test]
fn test_error_conversion() {
    let tool_err = ToolError::NotFound("test_tool".to_string());
    let mentalist_err: MentalistError = tool_err.into();
    
    match mentalist_err {
        MentalistError::ExecutorError(ToolError::NotFound(name)) => assert_eq!(name, "test_tool"),
        _ => panic!("Expected ExecutorError::NotFound"),
    }
}

#[test]
fn test_middleware_error_creation() {
    let err = MentalistError::MiddlewareError {
        middleware: "Auth".to_string(),
        source: anyhow::anyhow!("Unauthorized"),
    };
    
    assert!(err.to_string().contains("Middleware error in Auth"));
    assert!(err.to_string().contains("Unauthorized"));
}

#[test]
fn test_result_type() {
    fn produces_error() -> Result<()> {
        Err(MentalistError::AgentError("Failed".to_string()))
    }
    
    let res = produces_error();
    assert!(res.is_err());
}
