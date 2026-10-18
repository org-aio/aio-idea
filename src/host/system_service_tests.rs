//! 验证系统整包路由由安装记录提供，静态宿主不再暴露权限或租户接口。

use std::sync::Mutex;

use aio_plugin_identity_server::IdentityService;
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
    // 权限、租户、文件均由安装整包提供；未安装时静态宿主不暴露这些路由。
    for path in ["/api/rbac", "/api/tenants", "/api/files"] {
        assert_eq!(
            client
                .get(format!("{base}{path}"))
                .header("Cookie", cookie)
                .send()
                .await?
                .status(),
            404,
            "静态宿主不应暴露 {path}"
        );
    }
    assert_eq!(mcp(client, base, cookie).await?, 200);
    assert_eq!(grpc(client, base, cookie).await?, tonic::Code::Ok);

    // 提交无变化组合后身份服务保留同一实例，协议入口继续可用。
    apply(control, host, json!({"schema_version": 1})).await?;
    let restored = latest(generations)?;
    assert!(Arc::ptr_eq(
        &old.service::<IdentityService>()?,
        &restored.service::<IdentityService>()?
    ));
    assert_eq!(mcp(client, base, cookie).await?, 200);
    assert_eq!(grpc(client, base, cookie).await?, tonic::Code::Ok);
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
    Ok(())
}
