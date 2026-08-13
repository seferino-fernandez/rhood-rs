use std::{borrow::Cow, sync::Arc};

use axum::http::request::Parts;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode, Implementation,
    InitializeRequestParams, MetaObject, PaginatedRequestParams, ProtocolVersion,
    ServerCapabilities, Tool,
};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{ListToolsResult, ServerInfo},
    service::RequestContext,
};

use crate::middleware::oauth::{OAuthAuthenticated, OAuthPrincipal};
use crate::tools::pagination::CollectionPager;
use crate::tools::schema::{close_object_map, enforce_response_budget};

use super::handler::RhoodTools;
use super::rate_limit::{FallbackPrincipal, ToolClass};

const SERVER_INFO_META_KEY: &str = "io.modelcontextprotocol/serverInfo";
// Tool definitions carry full JSON schemas, so 20 materially bounds each
// response while keeping discovery to a handful of round trips.
pub(crate) const TOOLS_LIST_PAGE_SIZE: usize = 20;
const TOOLS_LIST_READ_ONLY_SCOPE: &str = "tools/list:read-only";
const TOOLS_LIST_WRITABLE_SCOPE: &str = "tools/list:writable";

/// The authorization and rate-limit class derived from a registered tool's
/// generated MCP annotations. Missing metadata fails closed as a write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolPolicy {
    Read,
    Write,
}

impl ToolPolicy {
    fn from_tool(tool: &Tool) -> Self {
        match tool
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint)
        {
            Some(true) => Self::Read,
            Some(false) | None => Self::Write,
        }
    }

    fn is_write(self) -> bool {
        self == Self::Write
    }

    fn rate_class(self) -> ToolClass {
        match self {
            Self::Read => ToolClass::Read,
            Self::Write => ToolClass::Write,
        }
    }
}

/// Adds the authoritative rhood-mcp identity to a tool result's existing metadata.
fn insert_server_info(meta: &mut Option<MetaObject>, server_info: &Implementation) {
    #[expect(
        clippy::expect_used,
        reason = "Implementation contains only infallibly serializable fields"
    )]
    let server_info = serde_json::to_value(server_info)
        .expect("rhood-mcp server information must serialize to JSON");
    meta.get_or_insert_default()
        .0
        .insert(SERVER_INFO_META_KEY.to_string(), server_info);
}

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
    policy: Option<ToolPolicy>,
    context: &RequestContext<RoleServer>,
) -> Option<McpError> {
    oauth_write_scope_error_for_parts(tool_name, policy, context.extensions.get::<Parts>())
}

fn oauth_write_scope_error_for_parts(
    tool_name: &str,
    policy: Option<ToolPolicy>,
    parts: Option<&Parts>,
) -> Option<McpError> {
    if !policy.is_some_and(ToolPolicy::is_write) {
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

fn rate_limit_principal(
    context: &RequestContext<RoleServer>,
    fallback: FallbackPrincipal,
) -> Result<String, McpError> {
    rate_limit_principal_for_parts(context.extensions.get::<Parts>(), fallback)
}

fn rate_limit_principal_for_parts(
    parts: Option<&Parts>,
    fallback: FallbackPrincipal,
) -> Result<String, McpError> {
    let oauth_authenticated = parts
        .and_then(|parts| parts.extensions.get::<OAuthAuthenticated>())
        .is_some();
    let authorization = parts.and_then(|parts| parts.extensions.get::<OAuthPrincipal>());

    match (oauth_authenticated, authorization) {
        (true, Some(authorization)) if !authorization.principal.is_empty() => {
            Ok(authorization.principal.clone())
        }
        (true, _) | (false, Some(_)) => Err(McpError::internal_error(
            "validated OAuth Principal is unavailable",
            None,
        )),
        (false, None) => fallback.as_str().map(str::to_string).ok_or_else(|| {
            McpError::internal_error("validated OAuth Principal is unavailable", None)
        }),
    }
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
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        tracing::debug!(read_only = self.read_only, "list_tools called");
        let mut tools: Vec<_> = if self.read_only {
            self.tool_router
                .list_all()
                .into_iter()
                .filter(|tool| !ToolPolicy::from_tool(tool).is_write())
                .collect()
        } else {
            self.tool_router.list_all()
        };
        for tool in &mut tools {
            close_tool_schemas(tool);
        }

        let scope = if self.read_only {
            TOOLS_LIST_READ_ONLY_SCOPE
        } else {
            TOOLS_LIST_WRITABLE_SCOPE
        };
        let cursor = request
            .as_ref()
            .and_then(|request| request.cursor.as_deref());
        let pager = CollectionPager::new(scope, cursor, Some(TOOLS_LIST_PAGE_SIZE))
            .map_err(|message| McpError::invalid_params(message, None))?;
        let (tools, next_cursor) = pager
            .page(tools)
            .map_err(|message| McpError::invalid_params(message, None))?;

        // SEP-2549 cache hints. The tool set is materialized from the router at
        // compile time and filtered by a `read_only` flag fixed at startup, so it
        // cannot change without a restart; a short TTL absorbs a client's startup
        // burst while bounding staleness across a redeploy.
        //
        // `Private` rather than `Public` because this list must never be served
        // from a shared cache to the wrong caller. It is invariant across callers
        // today, but the OAuth layer already distinguishes read and write scopes,
        // so per-token tool visibility is a short step away.
        let mut result = ListToolsResult::with_all_items(tools)
            .with_ttl_ms(300_000)
            .with_cache_scope(CacheScope::Private);
        result.next_cursor = next_cursor;
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        tracing::info!(tool = %request.name, "call_tool");
        let policy = self
            .tool_router
            .get(request.name.as_ref())
            .map(ToolPolicy::from_tool);
        if self.read_only && policy.is_some_and(ToolPolicy::is_write) {
            return Err(McpError::new(
                ErrorCode::INVALID_PARAMS,
                format!("Tool '{}' is disabled in read-only mode", request.name),
                None,
            ));
        }
        if let Some(error) = oauth_write_scope_error(&request.name, policy, &context) {
            return Err(error);
        }
        let tool_name = request.name.to_string();
        let server_info = self.get_info().server_info;
        // Unknown names still reach rmcp's dispatcher, but never receive the
        // more permissive read budget before it rejects them.
        let class = policy.unwrap_or(ToolPolicy::Write).rate_class();
        let principal = rate_limit_principal(&context, self.fallback_principal)?;
        match self.rate_limiter.check(&principal, class) {
            Ok(None) => {}
            Ok(Some(rejection)) => {
                tracing::warn!(
                    principal,
                    class = rejection.class.as_str(),
                    layer = rejection.layer.as_str(),
                    retry_after_seconds = rejection.retry_after_seconds,
                    "tool invocation rate limited"
                );
                // Exempt from `enforce_response_budget`: the payload is
                // fixed-size and server-generated, and a small configured
                // budget must not replace the structured rejection (and its
                // retry_after_seconds) with `response_too_large`.
                let mut result = CallToolResult::structured_error(serde_json::json!({
                    "error": "rate_limited",
                    "class": rejection.class.as_str(),
                    "layer": rejection.layer.as_str(),
                    "retry_after_seconds": rejection.retry_after_seconds,
                }));
                insert_server_info(&mut result.meta, &server_info);
                return Ok(result.into());
            }
            Err(error) => {
                tracing::error!(error = %error, "tool rate limiter failed closed");
                return Err(McpError::internal_error(error.to_string(), None));
            }
        }
        let tool_call_context = ToolCallContext::new(self, request, context);
        match self.tool_router.call(tool_call_context).await {
            Ok(CallToolResponse::Complete(result)) => {
                tracing::debug!(tool = %tool_name, "call_tool succeeded");
                let mut result = enforce_response_budget(result, self.max_response_bytes);
                insert_server_info(&mut result.meta, &server_info);
                Ok(result.into())
            }
            // `InputRequired` (MRTR) and `Task` carry no tool payload to size.
            Ok(CallToolResponse::InputRequired(mut result)) => {
                tracing::debug!(tool = %tool_name, "call_tool returned input required");
                insert_server_info(&mut result.meta, &server_info);
                Ok(result.into())
            }
            Ok(CallToolResponse::Task(mut result)) => {
                tracing::debug!(tool = %tool_name, "call_tool returned task");
                insert_server_info(&mut result.meta, &server_info);
                Ok(result.into())
            }
            // `CallToolResponse` is `#[non_exhaustive]`; future variants retain
            // their SDK-defined behavior until this boundary understands them.
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
        let mut tool = self.tool_router.get(name).cloned()?;
        if self.read_only && ToolPolicy::from_tool(&tool).is_write() {
            return None;
        }
        close_tool_schemas(&mut tool);
        Some(tool)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::http::Request;
    use rhood_core::{RhoodConfig, RobinhoodClient};
    use rmcp::{
        Peer, RoleServer, ServerHandler,
        handler::server::tool::{ToolRoute, ToolRouter},
        model::{
            CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
            CreateTaskResult, InputRequiredResult, ListToolsResult, MetaObject, NumberOrString,
            PaginatedRequestParams, Task, TaskStatus, Tool, ToolAnnotations,
        },
        service::{RequestContext, RunningService, serve_directly},
    };
    use serde_json::{Value, json};

    use super::{
        FallbackPrincipal, Implementation, McpError, OAuthAuthenticated, OAuthPrincipal, Parts,
        RhoodTools, SERVER_INFO_META_KEY, TOOLS_LIST_PAGE_SIZE, ToolPolicy, insert_server_info,
        oauth_write_scope_error_for_parts, rate_limit_principal_for_parts,
    };
    use crate::config::McpConfig;
    use crate::tools::ToolRateLimiter;

    /// Builds a `RhoodTools` with no network access.
    fn tools_with(read_only: bool) -> RhoodTools {
        let client = RobinhoodClient::with_config(RhoodConfig::default())
            .expect("default config builds a client");
        let hook = Arc::new(|_c: RobinhoodClient, _p: Peer<RoleServer>| {
            Box::pin(async { Ok(()) })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
        });
        let config = McpConfig::default();
        let rate_limiter =
            ToolRateLimiter::new(&config.rate_limit).expect("default rate limits are valid");
        RhoodTools::new_lazy(client, hook, read_only, &config, rate_limiter)
    }

    async fn list_page(
        tools: &RhoodTools,
        cursor: Option<String>,
    ) -> Result<ListToolsResult, McpError> {
        let request =
            cursor.map(|cursor| PaginatedRequestParams::default().with_cursor(Some(cursor)));
        let (server_transport, _client_transport) = tokio::io::duplex(4_096);
        let running: RunningService<RoleServer, RhoodTools> =
            serve_directly(tools.clone(), server_transport, None);
        let context = RequestContext::new(NumberOrString::Number(1), running.peer().clone());
        running.service().list_tools(request, context).await
    }

    fn oauth_parts(principal: Option<OAuthPrincipal>) -> Parts {
        let (mut parts, _body) = Request::new(()).into_parts();
        parts.extensions.insert(OAuthAuthenticated);
        if let Some(principal) = principal {
            parts.extensions.insert(principal);
        }
        parts
    }

    fn policy_for_registered_tool(tools: &RhoodTools, name: &str) -> ToolPolicy {
        tools
            .tool_router
            .get(name)
            .map(ToolPolicy::from_tool)
            .unwrap_or_else(|| panic!("{name} must be registered"))
    }

    fn meta_with_marker(marker: &str) -> Option<MetaObject> {
        Some(
            serde_json::from_value(json!({
                "unrelated": marker,
                SERVER_INFO_META_KEY: { "name": "not-rhood-mcp" }
            }))
            .expect("test metadata deserializes"),
        )
    }

    fn assert_server_info(meta: &Option<MetaObject>, marker: &str, server_info: &Implementation) {
        let meta: Value = serde_json::to_value(meta).expect("metadata serializes");
        assert_eq!(meta["unrelated"], marker);
        assert_eq!(
            meta[SERVER_INFO_META_KEY],
            serde_json::to_value(server_info).expect("server info serializes")
        );
    }

    #[test]
    fn every_tool_result_variant_carries_authoritative_server_info() {
        let server_info = tools_with(false).get_info().server_info;

        let mut complete = CallToolResult::default();
        complete.meta = meta_with_marker("complete");
        insert_server_info(&mut complete.meta, &server_info);
        assert_server_info(&complete.meta, "complete", &server_info);

        let mut input_required = InputRequiredResult::from_request_state("state");
        input_required.meta = meta_with_marker("input-required");
        insert_server_info(&mut input_required.meta, &server_info);
        assert_server_info(&input_required.meta, "input-required", &server_info);

        let task = Task::new(
            "task-id",
            TaskStatus::Working,
            "2026-08-10T00:00:00Z",
            "2026-08-10T00:00:00Z",
        );
        let mut task = CreateTaskResult::new(task);
        task.meta = meta_with_marker("task");
        insert_server_info(&mut task.meta, &server_info);
        assert_server_info(&task.meta, "task", &server_info);
    }

    #[tokio::test]
    async fn list_tools_pages_the_complete_sorted_surface() {
        let tools = tools_with(false);
        let expected: Vec<_> = tools
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        let mut actual = Vec::new();
        let mut page_lengths = Vec::new();
        let mut cursor = None;

        for _ in 0..10 {
            let result = list_page(&tools, cursor)
                .await
                .expect("each tools/list page succeeds");
            assert_eq!(result.ttl_ms, Some(300_000));
            assert_eq!(result.cache_scope, Some(CacheScope::Private));
            page_lengths.push(result.tools.len());
            actual.extend(result.tools.into_iter().map(|tool| tool.name.to_string()));
            cursor = result.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        assert_eq!(page_lengths.first(), Some(&TOOLS_LIST_PAGE_SIZE));
        assert!(
            page_lengths
                .iter()
                .all(|length| *length <= TOOLS_LIST_PAGE_SIZE)
        );
        assert_eq!(actual, expected);
        assert!(
            cursor.is_none(),
            "the final page must terminate the sequence"
        );
    }

    #[tokio::test]
    async fn list_tools_rejects_malformed_and_cross_surface_cursors() {
        let writable = tools_with(false);
        let malformed = list_page(&writable, Some("not-a-cursor".to_string()))
            .await
            .expect_err("malformed cursors must fail");
        assert_eq!(malformed.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(malformed.message.contains("pass next_cursor"));

        let writable_cursor = list_page(&writable, None)
            .await
            .expect("first writable page succeeds")
            .next_cursor
            .expect("writable surface has another page");
        let read_only = tools_with(true);
        let mismatched = list_page(&read_only, Some(writable_cursor))
            .await
            .expect_err("writable cursors must not address the read-only surface");
        assert_eq!(mismatched.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(mismatched.message.contains("does not belong"));
    }

    #[test]
    fn read_scope_rejects_write_tool() {
        let tools = tools_with(false);
        assert!(tools.get_tool("place_stock_order").is_some());
        let policy = policy_for_registered_tool(&tools, "place_stock_order");
        let parts = oauth_parts(Some(OAuthPrincipal {
            principal: "rhood_cid_read_only".to_string(),
            scopes: vec!["read".to_string()],
        }));

        let error =
            oauth_write_scope_error_for_parts("place_stock_order", Some(policy), Some(&parts));
        assert!(error.is_some(), "read scope must reject place_stock_order");
        let Some(error) = error else {
            return;
        };
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("place_stock_order"));
        assert!(error.message.contains("write"));
    }

    #[test]
    fn read_scope_allows_non_write_tool() {
        let tools = tools_with(false);
        let policy = policy_for_registered_tool(&tools, "get_account_profile");
        let parts = oauth_parts(Some(OAuthPrincipal {
            principal: "rhood_cid_read_only".to_string(),
            scopes: vec!["read".to_string()],
        }));

        assert!(
            oauth_write_scope_error_for_parts("get_account_profile", Some(policy), Some(&parts))
                .is_none()
        );
    }

    #[test]
    fn write_scope_allows_write_tool() {
        let tools = tools_with(false);
        let policy = policy_for_registered_tool(&tools, "place_stock_order");
        let parts = oauth_parts(Some(OAuthPrincipal {
            principal: "rhood_cid_read_write".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
        }));

        assert!(
            oauth_write_scope_error_for_parts("place_stock_order", Some(policy), Some(&parts))
                .is_none()
        );
    }

    #[test]
    fn oauth_request_without_principal_fails_closed_for_write_tool() {
        let tools = tools_with(false);
        let policy = policy_for_registered_tool(&tools, "place_stock_order");
        let parts = oauth_parts(None);

        assert!(
            oauth_write_scope_error_for_parts("place_stock_order", Some(policy), Some(&parts))
                .is_some()
        );
    }

    #[test]
    fn missing_policy_metadata_fails_closed_as_write() {
        let without_annotations = Tool::new(
            "missing_annotations",
            "Test tool with no annotations",
            Arc::new(Default::default()),
        );
        let without_hint = Tool::new(
            "missing_hint",
            "Test tool with no read-only hint",
            Arc::new(Default::default()),
        )
        .with_annotations(ToolAnnotations::new());

        assert_eq!(
            ToolPolicy::from_tool(&without_annotations),
            ToolPolicy::Write
        );
        assert_eq!(ToolPolicy::from_tool(&without_hint), ToolPolicy::Write);
    }

    #[tokio::test]
    async fn write_scope_allows_direct_call_tool_without_http_parts_to_dispatch() {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let counted_dispatches = Arc::clone(&dispatches);
        let mut tools = tools_with(false);
        tools.tool_router = ToolRouter::new().with_route(ToolRoute::new_dyn(
            Tool::new(
                "place_stock_order",
                "Count direct stdio dispatches",
                Arc::new(Default::default()),
            )
            .with_annotations(ToolAnnotations::new().read_only(false)),
            move |_context| {
                counted_dispatches.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    Ok(CallToolResult::success(vec![ContentBlock::text("called")]).into())
                })
            },
        ));
        let (server_transport, _client_transport) = tokio::io::duplex(4_096);
        let running: RunningService<RoleServer, RhoodTools> =
            serve_directly(tools, server_transport, None);
        let context = RequestContext::new(NumberOrString::Number(1), running.peer().clone());

        let response = running
            .service()
            .call_tool(CallToolRequestParams::new("place_stock_order"), context)
            .await
            .expect("direct stdio write-tool call is allowed");

        assert!(matches!(response, CallToolResponse::Complete(_)));
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn validated_oauth_client_id_is_the_rate_limit_principal() {
        let parts = oauth_parts(Some(OAuthPrincipal {
            principal: "registered-client".to_string(),
            scopes: vec!["read".to_string()],
        }));

        assert_eq!(
            rate_limit_principal_for_parts(Some(&parts), FallbackPrincipal::HttpOAuth)
                .expect("validated Principal is accepted"),
            "registered-client"
        );
    }

    #[test]
    fn bounded_transport_principals_cover_non_oauth_modes() {
        let (parts, _body) = Request::new(()).into_parts();
        assert_eq!(
            rate_limit_principal_for_parts(None, FallbackPrincipal::Stdio)
                .expect("stdio has a synthetic Principal"),
            "transport:stdio"
        );
        assert_eq!(
            rate_limit_principal_for_parts(Some(&parts), FallbackPrincipal::HttpToken)
                .expect("token auth has a synthetic Principal"),
            "transport:http-token"
        );
        assert_eq!(
            rate_limit_principal_for_parts(Some(&parts), FallbackPrincipal::HttpNone)
                .expect("no-auth has a synthetic Principal"),
            "transport:http-none"
        );
    }

    #[test]
    fn oauth_rate_limit_identity_fails_closed_without_validated_principal() {
        let parts = oauth_parts(None);
        let _missing_principal =
            rate_limit_principal_for_parts(Some(&parts), FallbackPrincipal::HttpOAuth)
                .expect_err("OAuth marker without a Principal must fail closed");

        let (plain_parts, _body) = Request::new(()).into_parts();
        let _missing_marker =
            rate_limit_principal_for_parts(Some(&plain_parts), FallbackPrincipal::HttpOAuth)
                .expect_err("OAuth transport without validated markers must fail closed");
    }
}
