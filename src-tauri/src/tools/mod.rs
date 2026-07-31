pub mod context;
pub mod dispatch;
pub mod exec;
pub mod file;
pub mod git;
pub mod history;
mod image_tool;
pub mod patch;
pub mod policy;
pub mod registry;
pub mod session;
pub mod workspace;

pub use context::{SharedToolContext, ToolContext};
/// 唯一工具执行入口；MCP 与 Actions 必须调用此函数，不得分叉实现。
pub use dispatch::call_tool;
pub use policy::{validate_actions_exposure, PolicySettings};
pub use registry::{
    exposed_tool_names, exposed_tool_names_for_context, is_allowed_tool,
    is_allowed_tool_for_context, is_mutating_tool_for_context, list_tools, list_tools_for_context,
    list_tools_for_profile, MUTATING_TOOLS,
};
pub use workspace::{wrap_mcp_tool_result, wrap_tool_result, Workspace};
