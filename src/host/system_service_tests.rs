//! 保留旧代数引用，验证路由停用确实释放资源，重新启用仍能读取原有业务数据。

use std::sync::Mutex;

use aio_plugin_file_server::{FileService, FileServiceImpl};
use aio_plugin_identity_server::IdentityService;
use aio_plugin_rbac_server::AccessControlService;
use aio_plugin_tenant_server::TenantService;
use az_plugin_host::runtime::server::RuntimeState;

use super::*;

pub(super) async fn verify(
    control: &mut RuntimeComposition,
    host: &RuntimeHost,
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    generations: &Mutex<Vec<RuntimeState>>,
) -> Result<()> {
    let old = latest(generations)?;
    let uploaded = client
        .post(format!("{base}/api/files"))
        .header("Cookie", cookie)
        .header("Content-Type", "text/plain")
        .query(&[("filename", "lifecycle.txt")])
        .body("persistent plugin data")
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let file_id = uploaded["data"]["id"].as_str().context("缺少文件 ID")?;
    let paths = [
        "/api/files",
        "/api/rbac",
        "/api/tenants",
        "/api/plugins/marketplace",
    ];
    for path in paths {
        assert_eq!(
            client
                .get(format!("{base}{path}"))
                .header("Cookie", cookie)
                .send()
                .await?
                .status(),
            200
        );
    }

    apply(
        control,
        host,
        json!({
            "schema_version": 1,
            "files": {"enabled": false},
            "access_control": {"enabled": false},
            "tenants": {"enabled": false},
            "marketplace": {"enabled": false},
        }),
    )
    .await?;
    for path in paths {
        assert_eq!(
            client
                .get(format!("{base}{path}"))
                .header("Cookie", cookie)
                .send()
                .await?
                .status(),
            404
        );
    }
    let disabled = latest(generations)?;
    assert!(Arc::ptr_eq(
        &old.service::<IdentityService>()?,
        &disabled.service::<IdentityService>()?
    ));
    assert_services_closed(&old).await?;
    assert_eq!(mcp(client, base, cookie).await?, 200);
    assert_eq!(grpc(client, base, cookie).await?, 200);

    apply(control, host, json!({"schema_version":1})).await?;
    let restored = latest(generations)?;
    assert!(!Arc::ptr_eq(
        &old.service::<FileServiceImpl>()?,
        &restored.service::<FileServiceImpl>()?
    ));
    let body = client
        .get(format!("{base}/api/files/{file_id}"))
        .header("Cookie", cookie)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert_eq!(body, "persistent plugin data");
    for path in paths {
        assert_eq!(
            client
                .get(format!("{base}{path}"))
                .header("Cookie", cookie)
                .send()
                .await?
                .status(),
            200
        );
    }
    for path in [format!("/api/files/{file_id}")] {
        client
            .delete(format!("{base}{path}"))
            .header("Cookie", cookie)
            .send()
            .await?
            .error_for_status()?;
    }
    Ok(())
}

pub(super) fn latest(generations: &Mutex<Vec<RuntimeState>>) -> Result<RuntimeState> {
    generations
        .lock()
        .map_err(|_| anyhow::anyhow!("代数记录锁损坏"))?
        .last()
        .cloned()
        .context("缺少运行时代数")
}

pub(super) async fn identity_rebuilt(old: &RuntimeState, next: &RuntimeState) -> Result<()> {
    assert!(!Arc::ptr_eq(
        &old.service::<IdentityService>()?,
        &next.service::<IdentityService>()?
    ));
    assert!(!Arc::ptr_eq(
        &old.service::<FileServiceImpl>()?,
        &next.service::<FileServiceImpl>()?
    ));
    assert!(!Arc::ptr_eq(
        &old.service::<AccessControlService>()?,
        &next.service::<AccessControlService>()?
    ));
    assert!(!Arc::ptr_eq(
        &old.service::<TenantService>()?,
        &next.service::<TenantService>()?
    ));
    assert_services_closed(old).await
}

async fn assert_services_closed(state: &RuntimeState) -> Result<()> {
    assert_pool_closed(state.service::<FileServiceImpl>()?.list("default").await)?;
    assert_pool_closed(
        state
            .service::<AccessControlService>()?
            .view("default", "probe")
            .await,
    )?;
    assert_pool_closed(
        state
            .service::<TenantService>()?
            .list("probe", "default")
            .await,
    )
}

fn assert_pool_closed<T>(result: Result<T>) -> Result<()> {
    let error = result.err().context("已停用服务仍可读取数据库")?;
    assert!(
        matches!(
            error.downcast_ref::<sqlx::Error>(),
            Some(sqlx::Error::PoolClosed)
        ),
        "{error:#}"
    );
    Ok(())
}
