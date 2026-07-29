use std::sync::Arc;

use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, ErrorCode, Implementation,
    PaginatedRequestParams, ProtocolVersion, ServerCapabilities, Tool,
};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{ListToolsResult, ServerInfo},
    service::RequestContext,
};

use crate::tools::schema::{close_object_map, enforce_response_budget};

use super::handler::RhoodTools;
use super::types::WRITE_TOOLS;

/// Hardens a tool's advertised schemas in place: closes the input schema and,
/// when present, the output schema (sets `additionalProperties: false` and an
/// explicit `required` on every object node).
///
/// Shared by `list_tools` and `get_tool` so the schema a client is shown and the
/// one rmcp resolves `Mcp-Param-*` headers against are the same document.
fn close_tool_schemas(tool: &mut Tool) {
    close_object_map(Arc::make_mut(&mut tool.input_schema));
    if let Some(output) = tool.output_schema.as_mut() {
        close_object_map(Arc::make_mut(output));
    }
}

impl ServerHandler for RhoodTools {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        // Pinned deliberately rather than using `ProtocolVersion::LATEST`, which
        // still resolves to 2025-11-25 in rmcp 3.0 and will move in a later
        // release. Naming the version keeps a future SDK bump from silently
        // changing what this server advertises; `advertises_current_protocol_version`
        // turns any such change into a reviewed diff.
        //
        // Note this value is only the fallback for clients requesting an unknown
        // version: `negotiate_protocol_version` echoes back any version in
        // `ProtocolVersion::KNOWN_VERSIONS`, so older clients still negotiate down.
        info.protocol_version = ProtocolVersion::V_2026_07_28;
        // No `enable_logging()`: SEP-2577 deprecated MCP logging, and rmcp 3.0
        // marks the whole surface `#[deprecated]`. Server-side diagnostics go to
        // `tracing` (stderr) instead.
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        // Not `Implementation::from_build_env()`: its `env!` calls expand inside
        // rmcp, so it reports the SDK's identity ("rmcp", 3.0.0) rather than this
        // server's. Expanding them here resolves them against this crate.
        info.server_info = Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
            .with_description(env!("CARGO_PKG_DESCRIPTION"));
        info.instructions = Some(include_str!("instructions.md").to_string());
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
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
            close_tool_schemas(tool);
        }

        // SEP-2549 cache hints. The tool set is materialized from the router at
        // compile time and filtered by a `read_only` flag fixed at startup, so it
        // cannot change without a restart; a short TTL absorbs a client's startup
        // burst while bounding staleness across a redeploy.
        //
        // `Private` rather than `Public` because this list must never be served
        // from a shared cache to the wrong caller. It is invariant across callers
        // today, but the OAuth layer already distinguishes read and write scopes,
        // so per-token tool visibility is a short step away.
        Ok(ListToolsResult::with_all_items(tools)
            .with_ttl_ms(300_000)
            .with_cache_scope(CacheScope::Private))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        tracing::info!(tool = %request.name, "call_tool");
        if self.read_only && WRITE_TOOLS.contains(&request.name.as_ref()) {
            return Err(McpError::new(
                ErrorCode::INVALID_PARAMS,
                format!("Tool '{}' is disabled in read-only mode", request.name),
                None,
            ));
        }
        let tool_name = request.name.to_string();
        let tool_call_context = ToolCallContext::new(self, request, context);
        match self.tool_router.call(tool_call_context).await {
            Ok(CallToolResponse::Complete(result)) => {
                tracing::debug!(tool = %tool_name, "call_tool succeeded");
                Ok(enforce_response_budget(result, self.max_response_bytes).into())
            }
            // `InputRequired` (MRTR) and `Task` carry no tool payload to size, so
            // they pass through untouched. The wildcard is required because
            // `CallToolResponse` is `#[non_exhaustive]`, and passing future
            // variants through unmodified is also the correct behavior.
            Ok(other) => {
                tracing::debug!(tool = %tool_name, "call_tool returned a non-terminal response");
                Ok(other)
            }
            Err(err) => {
                tracing::warn!(tool = %tool_name, error = %err.message, "call_tool failed");
                Err(err)
            }
        }
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        if self.read_only && WRITE_TOOLS.contains(&name) {
            return None;
        }
        let mut tool = self.tool_router.get(name).cloned()?;
        close_tool_schemas(&mut tool);
        Some(tool)
    }
}
