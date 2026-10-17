//! 每次发布运行时代数都重新装配应用路由，首页快照和权限不会固定在旧容器。

use std::path::PathBuf;

use anyhow::Result;
use axum::{
    Json, Router,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
    routing::{any, get},
};
use az_plugin_host::runtime::server::{RuntimeState, bootstrap_document};

pub(crate) fn router(state: &RuntimeState, runtime: Router, web_dist: PathBuf) -> Result<Router> {
    let frame_policy = HeaderValue::from_str(&state.frame_policy())?;
    let static_files = az_plugin_host::static_files::application(web_dist).layer(
        axum::middleware::from_fn_with_state(state.clone(), bootstrap_document),
    );
    let application = Router::new()
        .route("/api/{*path}", any(api_not_found))
        .fallback_service(static_files)
        .layer(axum::middleware::from_fn(missing_service));
    Ok(Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(runtime)
        .fallback_service(application)
        .layer(axum::middleware::map_response(
            move |mut response: axum::response::Response| {
                let frame_policy = frame_policy.clone();
                async move {
                    // 账户页面只允许本站及部署声明的局域网隔离文档。
                    response
                        .headers_mut()
                        .append(header::CONTENT_SECURITY_POLICY, frame_policy);
                    response
                }
            },
        )))
}

// 只有未匹配运行时路由的请求到达这里；缺失的 RPC 服务必须返回 gRPC 状态。
async fn missing_service(request: Request, next: Next) -> Response {
    let grpc = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value == "application/grpc" || value.starts_with("application/grpc+"));
    if grpc {
        return tonic::Status::unimplemented("RPC 服务未装配").into_http();
    }
    next.run(request).await
}

async fn api_not_found() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "API 路由不存在"})),
    )
}
