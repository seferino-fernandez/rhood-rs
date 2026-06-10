use std::sync::Arc;

use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, SetLevelRequestParams};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{ListToolsResult, ServerInfo},
    service::RequestContext,
};

use crate::tools::schema::{close_object_map, enforce_response_budget};

use super::handler::RhoodTools;
use super::types::WRITE_TOOLS;

impl ServerHandler for RhoodTools {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.protocol_version = ProtocolVersion::LATEST;
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_logging()
            .build();
        info.server_info = Implementation::from_build_env();
        info.instructions = Some(include_str!("instructions.md").to_string());
        info
    }

    async fn set_level(
        &self,
        request: SetLevelRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        tracing::debug!(level = ?request.level, "MCP log level change requested");
        let mut current = self.min_log_level.write().await;
        *current = request.level;
        tracing::info!(new_level = ?request.level, "MCP log level updated");
        Ok(())
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        tracing::debug!(read_only = self.read_only, "list_tools called");
        let mut tools: Vec<_> = if self.read_only {
            self.tool_router
                .list_all()
                .into_iter()
                .filter(|tool| !WRITE_TOOLS.contains(&tool.name.as_ref()))
                .collect()
        } else {
            self.tool_router.list_all()
        };
        for tool in &mut tools {
            close_object_map(Arc::make_mut(&mut tool.input_schema));
        }
        Ok(ListToolsResult {
            tools,
            meta: None,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResult, McpError> {
        tracing::info!(tool = %request.name, "call_tool");
        if self.read_only && WRITE_TOOLS.contains(&request.name.as_ref()) {
            return Err(McpError::new(
                rmcp::model::ErrorCode::INVALID_PARAMS,
                format!("Tool '{}' is disabled in read-only mode", request.name),
                None,
            ));
        }
        let tool_name = request.name.to_string();
        let tool_call_context =
            rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        let result = self.tool_router.call(tool_call_context).await;
        match result {
            Ok(call_result) => {
                tracing::debug!(tool = %tool_name, "call_tool succeeded");
                Ok(enforce_response_budget(
                    call_result,
                    self.max_response_bytes,
                ))
            }
            Err(err) => {
                tracing::warn!(tool = %tool_name, error = %err.message, "call_tool failed");
                Err(err)
            }
        }
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        if self.read_only && WRITE_TOOLS.contains(&name) {
            return None;
        }
        self.tool_router.get(name).cloned()
    }
}
