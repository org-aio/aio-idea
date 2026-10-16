//! 每次发布运行时代数都重新装配应用路由，首页快照和权限不会固定在旧容器。

use std::path::PathBuf;

use anyhow::Result;
use axum::{
    Json, Router,
    http::{HeaderValue, StatusCode, header},
    routing::{any, get},
};
use az_plugin_host::runtime::server::{RuntimeState, bootstrap_document};

pub(crate) fn router(
    state: &RuntimeState,
    runtime: Router,
    catalog: &dill::Catalog,
    web_dist: PathBuf,
) -> Result<Router> {
    let frame_policy = HeaderValue::from_str(&state.frame_policy())?;
    let static_files = az_plugin_host::static_files::application(web_dist).layer(
        axum::middleware::from_fn_with_state(state.clone(), bootstrap_document),
    );
    let application = Router::new()
        .route("/aio.runtime.v1.Enhancements/{*method}", any(api_not_found))
        .fallback_service(static_files);
    Ok(Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(runtime)
        .merge(crate::plugins::server_router(catalog)?)
        .route("/api/{*path}", any(api_not_found))
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

async fn api_not_found() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "API 路由不存在"})),
    )
}
