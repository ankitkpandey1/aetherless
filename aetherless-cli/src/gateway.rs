// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! HTTP Gateway for Aetherless.
//!
//! Acts as a reverse proxy, routing requests like `GET /functions/{id}` to the
//! appropriate local function instance's trigger port.
//!
//! Optimized for SMP using Axum/Tokio.

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    routing::{any, get},
    Router,
};
use reqwest::Client;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

use aetherless_core::{storage::Storage, FunctionRegistry};

/// Gateway state shared across threads
#[derive(Clone)]
struct GatewayState {
    registry: Arc<FunctionRegistry>,
    storage: Storage, // Thread-safe internally
    client: Client,
}

pub async fn start_gateway(
    port: u16,
    registry: Arc<FunctionRegistry>,
    storage: Storage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::builder().build()?;

    let state = GatewayState {
        registry,
        storage,
        client,
    };

    let app = Router::new()
        .route("/function/{function_id}/{*path}", any(proxy_handler))
        .route("/function/{function_id}", any(proxy_handler_root))
        .route("/storage/{key}", get(storage_get).put(storage_put))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Gateway listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn storage_get(
    State(state): State<GatewayState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    if let Some(val) = state.storage.get(&key) {
        (StatusCode::OK, val)
    } else {
        (StatusCode::NOT_FOUND, vec![])
    }
}

async fn storage_put(
    State(state): State<GatewayState>,
    Path(key): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    state.storage.put(key, body.to_vec());
    StatusCode::OK
}

async fn proxy_handler_root(
    State(state): State<GatewayState>,
    Path(function_id): Path<String>,
    req: Request<Body>,
) -> Result<impl IntoResponse, StatusCode> {
    proxy_request(state, function_id, req).await
}

async fn proxy_handler(
    State(state): State<GatewayState>,
    Path((function_id, _subpath)): Path<(String, String)>,
    req: Request<Body>,
) -> Result<impl IntoResponse, StatusCode> {
    proxy_request(state, function_id, req).await
}

async fn proxy_request(
    state: GatewayState,
    function_id: String,
    req: Request<Body>,
) -> Result<impl IntoResponse, StatusCode> {
    // 1. Lookup function in registry
    let fid =
        aetherless_core::FunctionId::new(&function_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let config = state.registry.get_config(&fid).map_err(|e| match e {
        aetherless_core::AetherError::FunctionNotFound(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    })?;

    let target_port = config.trigger_port.value();

    // 2. Rewrite URL
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|x| x.as_str())
        .unwrap_or("/");

    // Strip "/function/<id>" prefix
    let prefix = format!("/function/{}", function_id);
    let downstream_path = if let Some(stripped) = path_and_query.strip_prefix(&prefix) {
        if stripped.is_empty() {
            "/"
        } else {
            stripped
        }
    } else {
        path_and_query
    };

    let uri_string = format!("http://127.0.0.1:{}{}", target_port, downstream_path);

    // 3. Build downstream request using Reqwest
    let method = req.method().clone();
    let headers = req.headers().clone();

    // Convert Axum Body to Bytes
    let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // Create Reqwest request
    let mut downstream_req = state.client.request(method, &uri_string).body(body_bytes);

    // Copy headers (iterate by reference)
    for (name, value) in &headers {
        if name != "host" {
            downstream_req = downstream_req.header(name, value);
        }
    }

    // 4. Execute
    let resp = downstream_req.send().await.map_err(|e| {
        tracing::error!("Proxy error: {}", e);
        StatusCode::BAD_GATEWAY
    })?;

    // 5. Convert response back to Axum
    let status = resp.status();
    let mut builder = Response::builder().status(status);

    if let Some(headers_map) = builder.headers_mut() {
        for (name, value) in resp.headers() {
            headers_map.insert(name, value.clone());
        }
    }

    let resp_bytes = resp
        .bytes()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    builder
        .body(Body::from(resp_bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
