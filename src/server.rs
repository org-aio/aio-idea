use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use crate::plugins;
use anyhow::{Context as _, Result};
use az_plugin_host::runtime;

#[cfg(test)]
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
    let address = SocketAddr::from((host, port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("绑定监听地址失败: {address}"))?;
    let (config, bootstrap_components) = crate::host::configuration()?;
    // 在连接业务数据库前持久化候选；端口冲突则不会污染候选记录。
    let bootstrap = crate::host::RuntimeBootstrap::load(&config.cache_root)?;
    let mut runtime_plugins = runtime::server::plugins::default_plugins()?;
    runtime_plugins.extend(plugins::server::server_plugins()?);
    runtime_plugins.push(std::sync::Arc::new(crate::host::ProductIdentityPlugin));
    if !bootstrap_components.is_empty() {
        let bootstrap = runtime::server::plugins::ComponentBootstrapPlugin(bootstrap_components);
        runtime_plugins.push(std::sync::Arc::new(bootstrap));
    }
    let (runtime, mut composition) = bootstrap
        .start(config, runtime_plugins, move |state, runtime| {
            application::router(state, runtime, web_dist.clone())
        })
        .await?;
    let result = serve(&runtime, &mut composition, listener).await;
    let cleanup = runtime.shutdown().await;
    // 清理完成后才释放文件锁，恢复命令不能和旧运行时资源释放交错。
    drop(composition);
    match (result, cleanup) {
        (Err(error), Err(cleanup)) => Err(error.context(format!("运行时清理失败: {cleanup:#}"))),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), cleanup) => cleanup,
    }
}

async fn serve(
    runtime: &runtime::server::plugins::RuntimeHost,
    composition: &mut crate::host::RuntimeComposition,
    listener: tokio::net::TcpListener,
) -> Result<()> {
    let router = runtime.router();
    println!("AIO IDEA listening on http://{}", listener.local_addr()?);
    use std::future::IntoFuture;
    tokio::select! {
        result = axum::serve(listener, router).into_future() => {
            result.context("AIO IDEA 服务异常退出")
        }
        result = shutdown_signal() => result,
        result = composition.watch(runtime) => result,
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
