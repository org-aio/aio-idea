use anyhow::{Context, Result};
use az_plugin_host::configuration::{ComponentStorage, DeliveryConfig, HostConfig, PluginSource};
use az_plugin_host::runtime::server::plugins::BootstrapComponent;
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(serde::Deserialize)]
struct Composition {
    #[serde(default)]
    plugins: Vec<PluginSource>,
    #[serde(default)]
    bootstrap_components: Vec<BootstrapComponent>,
}

fn read_composition(path: &Path) -> Result<Composition> {
    let mut composition: Composition = toml::from_str(
        &std::fs::read_to_string(path)
            .with_context(|| format!("读取产品组合失败: {}", path.display()))?,
    )?;
    let directory = path.parent().context("产品配置路径缺少父目录")?;
    for component in &mut composition.bootstrap_components {
        if component.path.is_relative() {
            component.path = directory.join(&component.path);
        }
    }
    Ok(composition)
}

pub(super) fn cache_root() -> PathBuf {
    env::var_os("AIO_PLUGIN_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".aio/runtime".into())
}

pub fn configuration() -> Result<(HostConfig, Vec<BootstrapComponent>)> {
    let cache_root = cache_root();
    let path = env::var_os("AIO_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| "aio.toml".into());
    let composition = read_composition(&path)?;
    let host = HostConfig {
        development: None,
        remote_capabilities: env::var_os("AIO_REMOTE_CAPABILITIES").map(PathBuf::from),
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
                owner: env::var("AIO_DELIVERY_OWNER").unwrap_or_else(|_| "org-aio".into()),
                discovery_interval_seconds: 300,
                revision_interval_seconds: 60,
            }),
    };
    anyhow::ensure!(
        composition.bootstrap_components.is_empty() || host.component_storage.is_some(),
        "bootstrap_components 需要配置 AIO_COMPONENT_DATABASE_URL"
    );
    Ok((host, composition.bootstrap_components))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_paths_follow_configuration_location_and_require_explicit_fields() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("aio.toml");
        let manifest = format!(
            "plugins=[]\n[[bootstrap_components]]\npath='bundles/dictionary.aio-plugin'\ndigest='{}'\ntenants=['default']\n",
            "a".repeat(64)
        );
        std::fs::write(&path, &manifest)?;
        let composition = read_composition(&path)?;
        assert_eq!(
            composition.bootstrap_components[0].path,
            root.path().join("bundles/dictionary.aio-plugin")
        );
        assert_eq!(composition.bootstrap_components[0].tenants, ["default"]);
        for invalid in [
            manifest.replace("tenants=['default']", ""),
            format!("{manifest}tenant=['other']\n"),
        ] {
            std::fs::write(&path, invalid)?;
            assert!(read_composition(&path).is_err());
        }
        Ok(())
    }
}
