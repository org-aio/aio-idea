use super::{ProductIdentity, ProductIdentityPlugin};
use anyhow::{Result, ensure};
use az_plugin_core::{Plugin, PluginFuture};
use az_plugin_host::{
    authorization::AuthorizationProvider,
    configuration::HostConfig,
    identity::IdentityProvider,
    metering::MeteringProvider,
    runtime::server::{
        RuntimeState,
        plugins::{RuntimeAssembly, RuntimeHost},
    },
};
use std::{
    any::TypeId,
    sync::{Arc, Mutex},
};

struct FailAfterIdentity;
impl Plugin<RuntimeAssembly> for FailAfterIdentity {
    fn dependencies(&self) -> Vec<TypeId> {
        vec![TypeId::of::<ObserveIdentity>()]
    }
    fn build<'a>(&'a self, assembly: &'a mut RuntimeAssembly) -> PluginFuture<'a> {
        Box::pin(async move {
            assembly.on_initialize::<Self>(|_, _| {
                Box::pin(async { anyhow::bail!("测试后续插件启动失败") })
            });
            Ok(())
        })
    }
}

struct ObserveIdentity(Arc<Mutex<Vec<Arc<ProductIdentity>>>>);
impl Plugin<RuntimeAssembly> for ObserveIdentity {
    fn dependencies(&self) -> Vec<TypeId> {
        vec![TypeId::of::<ProductIdentityPlugin>()]
    }
    fn build<'a>(&'a self, assembly: &'a mut RuntimeAssembly) -> PluginFuture<'a> {
        Box::pin(async move {
            assembly.services.add_builder(self.0.clone());
            assembly.on_initialize::<Self>(|state, _| {
                Box::pin(async move {
                    let provider = state.service::<ProductIdentity>()?;
                    assert!(Arc::ptr_eq(
                        &provider.identity,
                        &state
                            .catalog()
                            .get_one::<aio_plugin_identity_server::IdentityService>()?
                    ));
                    let authentication: Arc<dyn IdentityProvider> = provider.clone();
                    let authorization: Arc<dyn AuthorizationProvider> = provider.clone();
                    let metering: Arc<dyn MeteringProvider> = provider.clone();
                    assert!(Arc::ptr_eq(
                        &state.service::<dyn IdentityProvider>()?,
                        &authentication
                    ));
                    assert!(Arc::ptr_eq(
                        &state.service::<dyn AuthorizationProvider>()?,
                        &authorization
                    ));
                    assert!(Arc::ptr_eq(
                        &state.service::<dyn MeteringProvider>()?,
                        &metering
                    ));
                    sqlx::query("SELECT 1").execute(&provider.pool).await?;
                    state
                        .service::<Mutex<Vec<Arc<ProductIdentity>>>>()?
                        .lock()
                        .map_err(|_| anyhow::anyhow!("观察锁损坏"))?
                        .push(provider);
                    Ok(())
                })
            });
            Ok(())
        })
    }
}

#[tokio::test]
#[ignore = "需要独立本机测试库并设置相同的 AIO_DATABASE_URL 与 AIO_TEST_DATABASE_URL"]
async fn product_identity_pool_is_closed_on_shutdown_and_failed_startup() -> Result<()> {
    let database = std::env::var("AIO_TEST_DATABASE_URL")?;
    ensure!(
        database.contains("127.0.0.1") && database.ends_with("/identity_plugin_test"),
        "只允许独立身份插件测试库"
    );
    ensure!(
        std::env::var("AIO_DATABASE_URL").as_deref() == Ok(database.as_str()),
        "身份插件需要同一测试库"
    );
    let root = tempfile::tempdir()?;
    for fail in [false, true] {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let config = HostConfig {
            remote_capabilities: None,
            database_url: database.clone(),
            public_origin: "http://127.0.0.1".into(),
            cache_root: root.path().join("cache"),
            component_storage: None,
            default_plugins: vec![],
            delivery: None,
            development: None,
        };
        let mut plugins: Vec<az_plugin_core::DynPlugin<RuntimeAssembly>> = vec![
            Arc::new(crate::plugins::server::IdentityServicePlugin),
            Arc::new(ProductIdentityPlugin),
            Arc::new(ObserveIdentity(observed.clone())),
        ];
        if fail {
            plugins.push(Arc::new(FailAfterIdentity));
        }
        let result = RuntimeState::initialize(config, plugins).await;
        let provider = observed
            .lock()
            .map_err(|_| anyhow::anyhow!("观察锁损坏"))?
            .last()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("身份未初始化"))?;
        if fail {
            assert!(result.is_err());
        } else {
            let state = result?;
            let authentication: Arc<dyn IdentityProvider> = provider.clone();
            let authorization: Arc<dyn AuthorizationProvider> = provider.clone();
            let metering: Arc<dyn MeteringProvider> = provider.clone();
            assert!(Arc::ptr_eq(
                &state.service::<dyn IdentityProvider>()?,
                &authentication
            ));
            assert!(Arc::ptr_eq(
                &state.service::<dyn AuthorizationProvider>()?,
                &authorization
            ));
            assert!(Arc::ptr_eq(
                &state.service::<dyn MeteringProvider>()?,
                &metering
            ));
            assert!(!provider.pool.is_closed());
            state.shutdown().await?;
        }
        assert!(provider.pool.is_closed());
        assert_identity_closed(&provider).await?;
        assert!(!root.path().join("cache").exists());
    }
    Ok(())
}

#[test]
fn publishing_requires_an_explicit_product_account() {
    use super::publish_account_allowed;
    assert!(!publish_account_allowed(None, "zjarlin"));
    assert!(publish_account_allowed(Some("alice, zjarlin"), "zjarlin"));
    assert!(!publish_account_allowed(
        Some("alice,zjarlin-admin"),
        "zjarlin"
    ));
}

#[tokio::test]
#[ignore = "需要独立本机身份插件测试库"]
async fn product_identity_restart_and_failed_replacement_create_fresh_pools() -> Result<()> {
    let database = std::env::var("AIO_TEST_DATABASE_URL")?;
    ensure!(
        database.contains("127.0.0.1") && database.ends_with("/identity_plugin_test"),
        "只允许独立身份插件测试库"
    );
    ensure!(
        std::env::var("AIO_DATABASE_URL").as_deref() == Ok(database.as_str()),
        "身份插件需要同一测试库"
    );
    let observed = Arc::new(Mutex::new(Vec::new()));
    let plugins: Vec<az_plugin_core::DynPlugin<RuntimeAssembly>> = vec![
        Arc::new(crate::plugins::server::IdentityServicePlugin),
        Arc::new(ProductIdentityPlugin),
        Arc::new(ObserveIdentity(observed.clone())),
    ];
    let root = tempfile::tempdir()?;
    let config = HostConfig {
        remote_capabilities: None,
        database_url: database,
        public_origin: "http://127.0.0.1".into(),
        cache_root: root.path().join("cache"),
        component_storage: None,
        default_plugins: vec![],
        delivery: None,
        development: None,
    };
    let host = RuntimeHost::initialize(config, plugins.clone()).await?;
    host.restart::<crate::plugins::server::IdentityServicePlugin>()
        .await?;
    let mut rejected = plugins;
    rejected[0] = Arc::new(crate::plugins::server::IdentityServicePlugin);
    rejected.push(Arc::new(FailAfterIdentity));
    assert!(host.reconfigure(rejected).await.is_err());
    {
        let providers = observed.lock().map_err(|_| anyhow::anyhow!("观察锁损坏"))?;
        assert_eq!(providers.len(), 4);
        assert!(
            providers[..3]
                .iter()
                .all(|provider| provider.pool.is_closed())
        );
        assert!(!providers[3].pool.is_closed());
        assert!(
            providers
                .windows(2)
                .all(|pair| !Arc::ptr_eq(&pair[0], &pair[1]))
        );
    }
    host.shutdown().await?;
    assert!(
        observed
            .lock()
            .map_err(|_| anyhow::anyhow!("观察锁损坏"))?
            .iter()
            .all(|provider| provider.pool.is_closed())
    );
    let providers = observed
        .lock()
        .map_err(|_| anyhow::anyhow!("观察锁损坏"))?
        .clone();
    assert!(
        providers
            .windows(2)
            .all(|pair| !Arc::ptr_eq(&pair[0].identity, &pair[1].identity))
    );
    for provider in providers {
        assert_identity_closed(&provider).await?;
    }
    Ok(())
}

async fn assert_identity_closed(provider: &ProductIdentity) -> Result<()> {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::COOKIE,
        "aio_session=lifecycle-closed-probe".parse()?,
    );
    let error = provider
        .identity
        .authenticate(&headers)
        .await
        .err()
        .ok_or_else(|| anyhow::anyhow!("旧身份服务仍能访问数据库"))?;
    assert!(
        matches!(
            error.downcast_ref::<sqlx::Error>(),
            Some(sqlx::Error::PoolClosed)
        ),
        "{error:#}"
    );
    Ok(())
}
