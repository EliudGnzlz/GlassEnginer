//! glass_mcp: Internal lightweight JSON-RPC MCP server for runtime engine introspection.

pub mod handlers;
pub mod protocol;

pub use handlers::McpRouter;
pub use protocol::{McpRequest, McpResponse};
