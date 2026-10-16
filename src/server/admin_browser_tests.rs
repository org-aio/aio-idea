use anyhow::{Context, Result, ensure};
use tower_http::services::{ServeDir, ServeFile};

#[tokio::test]
#[ignore = "需要本机独立测试数据库、文件目录、已构建的 Web 壳和 Playwright"]
async fn system_management_browser_workflows() -> Result<()> {
    let database = std::env::var("AIO_TEST_DATABASE_URL")?;
    let options: sqlx::postgres::PgConnectOptions = database.parse()?;
    ensure!(
        matches!(options.get_host(), "127.0.0.1" | "localhost" | "/tmp")
            && options.get_database() == Some("aio_keepalive_test"),
        "仅允许本机隔离测试数据库"
    );
    ensure!(
        std::env::var("AIO_DATABASE_URL")? == database,
        "数据库必须一致"
    );
    let dist = std::path::Path::new("target/dx/aio-idea/release/web/public");
    ensure!(dist.join("index.html").is_file(), "先构建 Web 壳");
    let port = std::env::var("AIO_ADMIN_PREVIEW_PORT").unwrap_or_else(|_| "0".into());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let base = format!("http://{}", listener.local_addr()?);
    let temporary = tempfile::tempdir()?;
    let config = az_plugin_host::configuration::HostConfig {
        database_url: database.clone(),
        public_origin: base.clone(),
        cache_root: temporary.path().join("cache"),
        component_storage: None,
        default_plugins: vec![],
        delivery: None,
        development: None,
        remote_capabilities: None,
    };
    let mut plugins = az_plugin_host::runtime::server::plugins::default_plugins()?;
    plugins.extend(crate::plugins::server::server_plugins()?);
    plugins.push(std::sync::Arc::new(crate::host::ProductIdentityPlugin));
    let runtime = az_plugin_host::runtime::server::plugins::RuntimeHost::initialize_with_router(
        config,
        plugins,
        move |_, router| {
            Ok(router.fallback_service(
                ServeDir::new(dist).fallback(ServeFile::new(dist.join("index.html"))),
            ))
        },
    )
    .await?;
    let router = runtime.router();
    println!("Isolated admin preview: {base}");
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    if port != "0" {
        let result = server.await?;
        runtime.shutdown().await?;
        result?;
        return Ok(());
    }
    let mut results = Vec::new();
    let test_run = format!("admin-test-{}", uuid::Uuid::new_v4().simple());
    for script in ["admin-files.cjs", "admin-rbac.cjs", "admin-account.cjs"] {
        let status = tokio::process::Command::new("node")
            .arg(format!("tests/browser/{script}"))
            .env("AIO_URL", &base)
            .env("AIO_ADMIN_TEST_RUN", &test_run)
            .status()
            .await
            .context("启动浏览器验收失败");
        results.push(status);
    }
    server.abort();
    let _ = server.await;
    runtime.shutdown().await?;
    let pool = sqlx::PgPool::connect(&database).await?;
    let tenants = sqlx::query_scalar::<_, String>("SELECT id FROM tenants WHERE label LIKE $1")
        .bind(format!("{test_run}%"))
        .fetch_all(&pool)
        .await?;
    for tenant in tenants {
        for statement in [
            "DELETE FROM auth_sessions WHERE tenant_id = $1",
            "DELETE FROM tenant_member_roles WHERE tenant_id = $1",
            "DELETE FROM role_permissions WHERE tenant_id = $1",
            "DELETE FROM tenant_memberships WHERE tenant_id = $1",
            "DELETE FROM tenants WHERE id = $1",
        ] {
            sqlx::query(statement).bind(&tenant).execute(&pool).await?;
        }
    }
    pool.close().await;
    for status in results {
        ensure!(status?.success(), "后台浏览器验收失败");
    }
    Ok(())
}
