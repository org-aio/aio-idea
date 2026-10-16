use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use crate::plugins;
use anyhow::{Context as _, Result};
use az_plugin_host::runtime;

#[cfg(test)]
mod admin_browser_tests;
pub(crate) mod application;
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
    let mut runtime_plugins = runtime::server::plugins::default_plugins()?;
    runtime_plugins.push(std::sync::Arc::new(crate::host::ProductIdentityPlugin(
        identity.clone(),
    )));
    let composition = crate::host::RuntimeComposition::load(runtime_plugins, identity).await?;
    let runtime = runtime::server::plugins::RuntimeHost::initialize_with_router(
        config,
        composition.plugins(),
        move |state, runtime| {
            application::router(state, runtime, &plugin_catalog, web_dist.clone())
        },
    )
    .await?;
    let result = serve(&runtime, composition, host, port).await;
    let cleanup = runtime.shutdown().await;
    match (result, cleanup) {
        (Err(error), Err(cleanup)) => Err(error.context(format!("运行时清理失败: {cleanup:#}"))),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), cleanup) => cleanup,
    }
}

async fn serve(
    runtime: &runtime::server::plugins::RuntimeHost,
    composition: crate::host::RuntimeComposition,
    host: IpAddr,
    port: u16,
) -> Result<()> {
    let router = runtime.router();
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
        result = composition.watch(runtime.clone()) => result,
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
