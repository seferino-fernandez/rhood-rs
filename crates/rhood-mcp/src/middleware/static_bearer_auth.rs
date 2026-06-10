use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub struct StaticAuthToken(pub String);

pub async fn static_bearer_auth(
    State(expected): State<StaticAuthToken>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    tracing::debug!(%method, %uri, "static_bearer_auth: incoming request");

    let header = req
        .headers()
        .get("authorization")
        .and_then(|header_value| header_value.to_str().ok())
        .and_then(|auth_header| auth_header.strip_prefix("Bearer "))
        .unwrap_or("");

    let expected_bytes = expected.0.as_bytes();
    let provided_bytes = header.as_bytes();

    if expected_bytes.len() != provided_bytes.len()
        || expected_bytes.ct_eq(provided_bytes).unwrap_u8() != 1
    {
        tracing::warn!(%method, %uri, "static_bearer_auth: rejected — invalid token");
        return Err(StatusCode::UNAUTHORIZED);
    }

    tracing::debug!(%method, %uri, "static_bearer_auth: accepted");
    Ok(next.run(req).await)
}
