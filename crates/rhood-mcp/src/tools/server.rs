use std::{borrow::Cow, sync::Arc};

use axum::http::request::Parts;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, ErrorCode, Implementation,
    InitializeRequestParams, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, Tool,
};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{ListToolsResult, ServerInfo},
    service::RequestContext,
};

use crate::middleware::oauth::{OAuthAuthenticated, OAuthPrincipal};
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

/// Returns the scope error for a write tool invoked through the OAuth transport.
///
/// Requests without HTTP parts are stdio, and requests whose HTTP parts lack
/// both OAuth markers use `token` or `none` authentication. Those paths have
/// no per-request OAuth scope and retain their existing behavior. Once either
/// marker is present, a missing or malformed carrier denies the write tool.
fn oauth_write_scope_error(
    tool_name: &str,
    context: &RequestContext<RoleServer>,
) -> Option<McpError> {
    oauth_write_scope_error_for_parts(tool_name, context.extensions.get::<Parts>())
}

fn oauth_write_scope_error_for_parts(tool_name: &str, parts: Option<&Parts>) -> Option<McpError> {
    if !WRITE_TOOLS.contains(&tool_name) {
        return None;
    }

    let parts = parts?;
    let oauth_authenticated = parts.extensions.get::<OAuthAuthenticated>().is_some();
    let authorization = parts.extensions.get::<OAuthPrincipal>();
    if !oauth_authenticated && authorization.is_none() {
        return None;
    }

    let has_write_scope = authorization.is_some_and(|authorization| {
        !authorization.principal.is_empty()
            && authorization.scopes.iter().any(|scope| scope == "write")
    });
    if has_write_scope {
        return None;
    }

    Some(McpError::new(
        ErrorCode::INVALID_PARAMS,
        format!("Tool '{tool_name}' requires the 'write' OAuth scope"),
        None,
    ))
}

impl ServerHandler for RhoodTools {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        // Pinned deliberately rather than using `ProtocolVersion::LATEST`, which
        // still resolves to 2025-11-25 in rmcp 3.1 and will move in a later
        // release. Naming the version keeps a future SDK bump from silently
        // changing what this server advertises; `advertises_current_protocol_version`
        // turns any such change into a reviewed diff.
        //
        // This is the sole revision the server supports; `initialize` rejects
        // every other requested version rather than negotiating down.
        info.protocol_version = ProtocolVersion::V_2026_07_28;
        // No `enable_logging()`: SEP-2577 deprecated MCP logging, and rmcp 3.1
        // marks the whole surface `#[deprecated]`. Server-side diagnostics go to
        // `tracing` (stderr) instead.
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        // Not `Implementation::from_build_env()`: its `env!` calls expand inside
        // rmcp, so it reports the SDK's identity ("rmcp", 3.1.2) rather than this
        // server's. Expanding them here resolves them against this crate.
        info.server_info = Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
            .with_description(env!("CARGO_PKG_DESCRIPTION"));
        info.instructions = Some(include_str!("instructions.md").to_string());
        info
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::InitializeResult, McpError> {
        if request.protocol_version != ProtocolVersion::V_2026_07_28 {
            return Err(McpError::unsupported_protocol_version(
                request.protocol_version,
                &[ProtocolVersion::V_2026_07_28],
            ));
        }

        // This is `ServerHandler`'s default initialize behavior after the
        // strict version check above. The trait has no super-call syntax.
        context.peer.set_peer_info(request.clone());
        let mut info = self.get_info();
        info.protocol_version = request.protocol_version;
        Ok(info)
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
        if let Some(error) = oauth_write_scope_error(&request.name, &context) {
            return Err(error);
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::Request;
    use rhood_core::{RhoodConfig, RobinhoodClient};
    use rmcp::{Peer, RoleServer, ServerHandler};

    use super::{
        OAuthAuthenticated, OAuthPrincipal, Parts, RhoodTools, oauth_write_scope_error_for_parts,
    };
    use crate::config::McpConfig;

    /// Builds a `RhoodTools` with no network access.
    fn tools_with(read_only: bool) -> RhoodTools {
        let client = RobinhoodClient::with_config(RhoodConfig::default())
            .expect("default config builds a client");
        let hook = Arc::new(|_c: RobinhoodClient, _p: Peer<RoleServer>| {
            Box::pin(async { Ok(()) })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
        });
        RhoodTools::new_lazy(client, hook, read_only, &McpConfig::default())
    }

    fn oauth_parts(principal: Option<OAuthPrincipal>) -> Parts {
        let (mut parts, _body) = Request::new(()).into_parts();
        parts.extensions.insert(OAuthAuthenticated);
        if let Some(principal) = principal {
            parts.extensions.insert(principal);
        }
        parts
    }

    #[test]
    fn read_scope_rejects_write_tool() {
        let tools = tools_with(false);
        assert!(tools.get_tool("confirm_order").is_some());
        let parts = oauth_parts(Some(OAuthPrincipal {
            principal: "rhood_cid_read_only".to_string(),
            scopes: vec!["read".to_string()],
        }));

        let error = oauth_write_scope_error_for_parts("confirm_order", Some(&parts));
        assert!(error.is_some(), "read scope must reject confirm_order");
        let Some(error) = error else {
            return;
        };
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("confirm_order"));
        assert!(error.message.contains("write"));
    }

    #[test]
    fn read_scope_allows_non_write_tool() {
        let parts = oauth_parts(Some(OAuthPrincipal {
            principal: "rhood_cid_read_only".to_string(),
            scopes: vec!["read".to_string()],
        }));

        assert!(oauth_write_scope_error_for_parts("get_account_profile", Some(&parts)).is_none());
    }

    #[test]
    fn write_scope_allows_write_tool() {
        let parts = oauth_parts(Some(OAuthPrincipal {
            principal: "rhood_cid_read_write".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
        }));

        assert!(oauth_write_scope_error_for_parts("confirm_order", Some(&parts)).is_none());
    }

    #[test]
    fn oauth_request_without_principal_fails_closed_for_write_tool() {
        let parts = oauth_parts(None);

        assert!(oauth_write_scope_error_for_parts("confirm_order", Some(&parts)).is_some());
    }
}
