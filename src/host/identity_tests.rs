use super::{ProductIdentity, ProductIdentityPlugin};
use anyhow::{Result, ensure};
use az_plugin_core::{Plugin, PluginFuture};
use az_plugin_host::{
    authorization::AuthorizationProvider,
    configuration::HostConfig,
    identity::IdentityProvider,
    metering::MeteringProvider,
    runtime::server::{RuntimeState, plugins::RuntimeAssembly},
};
use std::{any::TypeId, sync::Arc};

struct FailAfterIdentity;
impl Plugin<RuntimeAssembly> for FailAfterIdentity {
    fn dependencies(&self) -> Vec<TypeId> {
        vec![TypeId::of::<ProductIdentityPlugin>()]
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
    let mut catalog = dill::Catalog::builder();
    aio_plugin_identity_server::register(&mut catalog)?;
    let identity = aio_plugin_identity_server::service(&catalog.build())?;
    let root = tempfile::tempdir()?;
    for fail in [false, true] {
        let provider = Arc::new(ProductIdentity::new(identity.clone(), &database)?);
        let config = HostConfig {
            remote_capabilities: None,
            database_url: "unused".into(),
            public_origin: "http://127.0.0.1".into(),
            cache_root: root.path().join("cache"),
            component_storage: None,
            default_plugins: vec![],
            delivery: None,
            development: None,
        };
        let mut plugins: Vec<az_plugin_core::DynPlugin<RuntimeAssembly>> =
            vec![Arc::new(ProductIdentityPlugin(provider.clone()))];
        if fail {
            plugins.push(Arc::new(FailAfterIdentity));
        }
        let result = RuntimeState::initialize(config, plugins).await;
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
