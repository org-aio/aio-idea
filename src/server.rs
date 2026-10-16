use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use anyhow::{Context as _, Result};
use axum::{
    Json, Router,
    http::{HeaderValue, StatusCode, header},
    routing::{any, get},
};

use crate::plugins;
use az_plugin_host::runtime;

#[cfg(test)]
mod admin_browser_tests;
#[cfg(test)]
mod runtime_tests;

pub async fn run() -> Result<()> {
    let port = env::var("AIO_WEB_PORT")
        .unwrap_or_else(|_| "8080".to_owned())
        .parse::<u16>()
        .context("AIO_WEB_PORT 必须是有效端口")?;
    let host = env::var("AIO_WEB_HOST")
        .unwrap_or_else(|_| "127.0.0.1".to_owned())
        .parse::<IpAddr>()
        .context("AIO_WEB_HOST 必须是有效 IP 地址")?;
    let web_dist = env::var_os("AIO_WEB_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/dx/aio-idea/release/web/public"));
    let plugin_catalog = plugins::server_catalog()?;
    let identity = aio_plugin_identity_server::service(&plugin_catalog)?;
    identity.initialize().await?;
    aio_plugin_dictionary_server::service(&plugin_catalog)?
        .initialize()
        .await?;
    aio_plugin_file_server::service(&plugin_catalog)?
        .initialize()
        .await?;
    aio_plugin_rbac_server::service(&plugin_catalog)?
        .initialize()
        .await?;
    let config = crate::host::configuration()?;
    let identity = std::sync::Arc::new(
        crate::host::ProductIdentity::new(identity, &config.database_url).await?,
    );
    let runtime = runtime::server::RuntimeState::initialize(config, identity).await?;
    let result = serve(&runtime, &plugin_catalog, web_dist, host, port).await;
    let cleanup = runtime.shutdown().await;
    match (result, cleanup) {
        (Err(error), Err(cleanup)) => Err(error.context(format!("运行时清理失败: {cleanup:#}"))),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), cleanup) => cleanup,
    }
}

async fn serve(
    runtime: &runtime::server::RuntimeState,
    plugin_catalog: &dill::Catalog,
    web_dist: PathBuf,
    host: IpAddr,
    port: u16,
) -> Result<()> {
    let frame_policy = HeaderValue::from_str(&runtime.frame_policy())?;
    let application = az_plugin_host::static_files::application(web_dist).layer(
        axum::middleware::from_fn_with_state(runtime.clone(), runtime::server::bootstrap_document),
    );
    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(runtime::server::router(runtime.clone()))
        .merge(plugins::server_router(plugin_catalog)?)
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
        ));
    let address = SocketAddr::from((host, port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("绑定监听地址失败: {address}"))?;
    println!("AIO IDEA listening on http://{}", listener.local_addr()?);
    use std::future::IntoFuture;
    tokio::select! {
        result = axum::serve(listener, router).into_future() => {
            result.context("AIO IDEA 服务异常退出")
        }
        result = shutdown_signal() => result,
    }
}

// SIGTERM 与 Ctrl-C 都先停止监听，再等待运行时任务和资源释放。
async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("注册终止信号失败")?;
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal.context("等待退出信号失败")?,
            _ = terminate.recv() => {},
        }
        Ok(())
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.context("等待退出信号失败")
}

async fn api_not_found() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "API 路由不存在" })),
    )
}
