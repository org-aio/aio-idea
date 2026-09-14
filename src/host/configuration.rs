use anyhow::{Context, Result};
use az_plugin_host::configuration::{ComponentStorage, DeliveryConfig, HostConfig, PluginSource};
use std::{env, path::PathBuf};

pub fn configuration() -> Result<HostConfig> {
    let cache_root = env::var_os("AIO_PLUGIN_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".aio/runtime".into());
    let path = env::var_os("AIO_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| "aio.toml".into());
    #[derive(serde::Deserialize)]
    struct Composition {
        #[serde(default)]
        plugins: Vec<PluginSource>,
    }
    let composition: Composition = toml::from_str(
        &std::fs::read_to_string(&path)
            .with_context(|| format!("读取产品组合失败: {}", path.display()))?,
    )?;
    Ok(HostConfig {
        development: None,
        database_url: env::var("AIO_DATABASE_URL").context("缺少 AIO_DATABASE_URL")?,
        public_origin: env::var("AIO_PUBLIC_ORIGIN")
            .unwrap_or_else(|_| "https://aio.addzero.site".into()),
        component_storage: env::var("AIO_COMPONENT_DATABASE_URL")
            .ok()
            .map(|database_url| ComponentStorage {
                database_url,
                root: env::var_os("AIO_COMPONENT_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cache_root.join("components")),
            }),
        cache_root,
        default_plugins: composition.plugins,
        delivery: env::var("AIO_DELIVERY_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|_| DeliveryConfig {
                owner: env::var("AIO_DELIVERY_OWNER").unwrap_or_else(|_| "zjarlin".into()),
                discovery_interval_seconds: 300,
                revision_interval_seconds: 60,
            }),
    })
}
