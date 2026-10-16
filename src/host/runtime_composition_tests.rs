//! 使用产品路由、真实身份会话与 PostgreSQL 验证部署组合的在线提交和恢复。

use super::*;
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "需要本机独立身份测试库、临时文件目录和测试账户"]
async fn deployment_composition_switches_protocols_and_rebuilds_identity_without_rebinding()
-> Result<()> {
    let database = std::env::var("AIO_TEST_DATABASE_URL")?;
    let options: sqlx::postgres::PgConnectOptions = database.parse()?;
    ensure!(
        options.get_host() == "127.0.0.1" && options.get_database() == Some("identity_plugin_test"),
        "仅允许本机独立测试库"
    );
    ensure!(
        std::env::var("AIO_DATABASE_URL")? == database,
        "测试库必须一致"
    );
    let catalog = crate::plugins::server_catalog()?;
    let identity = aio_plugin_identity_server::service(&catalog)?;
    identity.initialize().await?;
    aio_plugin_dictionary_server::service(&catalog)?
        .initialize()
        .await?;
    aio_plugin_file_server::service(&catalog)?
        .initialize()
        .await?;
    aio_plugin_rbac_server::service(&catalog)?
        .initialize()
        .await?;
    let root = tempfile::tempdir()?;
    let web = root.path().join("web");
    tokio::fs::create_dir_all(&web).await?;
    tokio::fs::write(
        web.join("index.html"),
        "<!doctype html><html><head><title>组合测试</title></head><body></body></html>",
    )
    .await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let config = az_plugin_host::configuration::HostConfig {
        database_url: database,
        public_origin: base.clone(),
        cache_root: root.path().join("cache"),
        component_storage: None,
        default_plugins: vec![],
        delivery: None,
        development: None,
        remote_capabilities: None,
    };
    let mut plugins = az_plugin_host::runtime::server::plugins::default_plugins()?;
    plugins.push(Arc::new(ProductIdentityPlugin(identity.clone())));
    let path = root.path().join("composition.json");
    tokio::fs::write(&path, br#"{"schema_version":1}"#).await?;
    let bootstrap = RuntimeBootstrap::open(Some(path.clone()), &config.cache_root)?;
    let (host, mut control) = bootstrap
        .start(config, plugins, identity, move |state, runtime| {
            crate::server::application::router(state, runtime, &catalog, web.clone())
        })
        .await?;
    let router = host.router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let guard = ServerGuard(server);
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let login = client
        .post(format!("{base}/api/auth/login"))
        .json(&json!({
            "account": std::env::var("AIO_BOOTSTRAP_ACCOUNT")?,
            "password": std::env::var("AIO_BOOTSTRAP_PASSWORD")?,
        }))
        .send()
        .await?
        .error_for_status()?;
    let cookie = login
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .context("登录没有会话")?
        .to_str()?
        .split(';')
        .next()
        .context("会话为空")?
        .to_owned();
    assert_eq!(mcp(&client, &base, &cookie).await?, 200);
    assert_eq!(grpc(&client, &base, &cookie).await?, 200);
    assert_snapshot(&client, &base, &cookie).await?;

    apply(
        &mut control,
        &host,
        json!({"schema_version":1,"mcp":{"enabled":false}}),
    )
    .await?;
    assert_eq!(mcp(&client, &base, &cookie).await?, 404);
    assert_eq!(grpc(&client, &base, &cookie).await?, 200);
    apply(
        &mut control,
        &host,
        json!({"schema_version":1,"grpc":{"enabled":false},"identity":{"restart":1}}),
    )
    .await?;
    assert_eq!(mcp(&client, &base, &cookie).await?, 200);
    assert_eq!(grpc(&client, &base, &cookie).await?, 404);
    assert_snapshot(&client, &base, &cookie).await?;

    let before = control.plugins.clone();
    assert!(
        apply(&mut control, &host, json!({"schema_version":2}))
            .await
            .is_err()
    );
    assert!(
        apply(
            &mut control,
            &host,
            json!({"schema_version":1,"unknown":true})
        )
        .await
        .is_err()
    );
    assert!(
        before
            .iter()
            .zip(&control.plugins)
            .all(|(left, right)| Arc::ptr_eq(left, right))
    );
    assert_eq!(mcp(&client, &base, &cookie).await?, 200);
    assert_eq!(grpc(&client, &base, &cookie).await?, 404);

    apply(&mut control, &host, json!({"schema_version":1,"rhai":{"restart":1},"typescript":{"restart":1},"database":{"restart":1},"identity":{"restart":2}})).await?;
    assert_eq!(mcp(&client, &base, &cookie).await?, 200);
    assert_eq!(grpc(&client, &base, &cookie).await?, 200);
    assert_snapshot(&client, &base, &cookie).await?;
    tokio::fs::write(
        root.path().join("pending.json"),
        br#"{"schema_version":1,"mcp":{"enabled":false}}"#,
    )
    .await?;
    tokio::fs::rename(root.path().join("pending.json"), &path).await?;
    tokio::select! {
        result = control.watch(&host) => {
            anyhow::bail!("观察任务提前退出: {result:?}");
        }
        result = tokio::time::timeout(Duration::from_secs(10), async {
            while !matches!(mcp(&client, &base, &cookie).await, Ok(status) if status == 404) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            anyhow::Ok(())
        }) => result??,
    }
    let directory = root.path().join("cache/runtime-composition");
    assert!(ConfigurationJournal::open(&directory, &path).is_err());
    host.shutdown().await?;
    assert!(ConfigurationJournal::open(&directory, &path).is_err());
    drop(control);
    assert!(ConfigurationJournal::open(&directory, &path).is_ok());
    assert!(!host.is_available());
    assert_eq!(
        client.get(format!("{base}/health")).send().await?.status(),
        503
    );
    drop(guard);
    Ok(())
}

async fn apply(control: &mut RuntimeComposition, host: &RuntimeHost, value: Value) -> Result<()> {
    control.apply(host, serde_json::to_vec(&value)?).await
}

async fn mcp(client: &reqwest::Client, base: &str, cookie: &str) -> Result<reqwest::StatusCode> {
    Ok(client
        .post(format!("{base}/api/runtime/mcp"))
        .header("Cookie", cookie)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-06-18")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .status())
}

async fn grpc(client: &reqwest::Client, base: &str, cookie: &str) -> Result<reqwest::StatusCode> {
    let response = client
        .post(format!("{base}/aio.runtime.v1.Enhancements/List"))
        .header("Cookie", cookie)
        .header("Content-Type", "application/grpc")
        .body(vec![0; 5])
        .send()
        .await?;
    if response.status() == 200 {
        assert_eq!(
            response
                .headers()
                .get("Content-Type")
                .context("缺少协议类型")?,
            "application/grpc"
        );
    }
    Ok(response.status())
}

async fn assert_snapshot(client: &reqwest::Client, base: &str, cookie: &str) -> Result<()> {
    let response = client
        .get(base)
        .header("Cookie", cookie)
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        response
            .headers()
            .get("Cache-Control")
            .context("缺少缓存策略")?,
        "private, no-store"
    );
    assert!(response.headers().contains_key("Content-Security-Policy"));
    assert!(
        response
            .text()
            .await?
            .contains("id=\"aio-startup-snapshot\"")
    );
    Ok(())
}

struct ServerGuard(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}
