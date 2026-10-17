//! 系统服务与运行时共用 Dill 和生命周期；关闭后的资源不得带入下一代。

use std::sync::Arc;

use aio_plugin_file_server::{FileController, FileService, FileServiceImpl};
use aio_plugin_identity_server::IdentityService;
use aio_plugin_rbac_server::AccessControlService;
use aio_plugin_tenant_server::TenantService;
use anyhow::Result;
use az_plugin_core::{DynPlugin, Plugin, PluginFuture};
use az_plugin_host::runtime::server::plugins::{Resource, RuntimeAssembly};

// 每项服务声明资源、接口、控制器与依赖，复用同一套初始化和反序关闭步骤。
macro_rules! service_plugin {
    ($plugin:ident, $service:ty, [$($interface:ty),*], [$($controller:ty),*], [$($required:ty),*], $routes:path) => {
        /// 产品系统服务的原生插件，资源和路由跟随运行时代数。
        #[dill::component]
        #[dill::interface(dyn Plugin<RuntimeAssembly>)]
        pub struct $plugin;

        impl Plugin<RuntimeAssembly> for $plugin {
            fn build<'a>(&'a self, assembly: &'a mut RuntimeAssembly) -> PluginFuture<'a> {
                Box::pin(async move {
                    assembly.declare_resource::<$service>()?;
                    $(assembly.services.bind::<$interface, $service>();)*
                    $(assembly.services.add::<$controller>();)*
                    $(assembly.require_service::<$required>();)*
                    assembly.add_routes(|state| Ok($routes(state.catalog())?.with_state(())));
                    assembly.on_initialize::<Self>(|state, _| {
                        Box::pin(async move {
                            let service = Arc::new(<$service>::from_env()?);
                            state.service::<Resource<$service>>()?.provide(service.clone())?;
                            service.initialize().await
                        })
                    });
                    assembly.on_stop::<Self>(|state, _| {
                        Box::pin(async move {
                            if let Some(service) = state.service::<Resource<$service>>()?.initialized() {
                                service.shutdown().await;
                            }
                            Ok(())
                        })
                    });
                    Ok(())
                })
            }
        }
    };
}

service_plugin!(
    IdentityServicePlugin,
    IdentityService,
    [],
    [],
    [],
    aio_plugin_identity_server::router
);
service_plugin!(
    FilePlugin,
    FileServiceImpl,
    [dyn FileService],
    [FileController],
    [IdentityService],
    aio_plugin_file_server::router
);
service_plugin!(
    AccessControlPlugin,
    AccessControlService,
    [],
    [],
    [IdentityService],
    aio_plugin_rbac_server::router
);
service_plugin!(
    TenantPlugin,
    TenantService,
    [],
    [],
    [IdentityService],
    aio_plugin_tenant_server::router
);

/// 产品只选择具体插件类型，服务创建和初始化全部交给 RuntimeHost。
pub fn server_plugins() -> Result<Vec<DynPlugin<RuntimeAssembly>>> {
    let mut builder = dill::CatalogBuilder::new();
    builder
        .add::<IdentityServicePlugin>()
        .add::<FilePlugin>()
        .add::<AccessControlPlugin>()
        .add::<TenantPlugin>()
        .add::<aio_plugin_device_server::DeviceTunnelPlugin>();
    let catalog = builder.build();
    let plugins = catalog.get::<dill::AllOf<dyn Plugin<RuntimeAssembly>>>()?;
    Ok(plugins.into_iter().collect())
}
