//! 部署方声明原生能力组合；文件更新由监听器所有者处理，不进入待排空的插件任务。

use std::{
    any::TypeId,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use az_plugin_core::{DynPlugin, Plugin};
use az_plugin_host::runtime::server::plugins::{
    DatabasePlugin, EvolutionPlugin, GrpcPlugin, McpPlugin, RhaiPlugin, RuntimeAssembly,
    RuntimeHost, TypeScriptPlugin,
};
use serde::Deserialize;
use tokio::io::AsyncReadExt;

use super::ProductIdentityPlugin;

const MAX_COMPOSITION_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
struct Capability {
    enabled: bool,
    restart: u64,
}

impl Default for Capability {
    fn default() -> Self {
        Self {
            enabled: true,
            restart: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
struct Resource {
    restart: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Composition {
    schema_version: u32,
    #[serde(default)]
    rhai: Capability,
    #[serde(default)]
    typescript: Capability,
    #[serde(default)]
    mcp: Capability,
    #[serde(default)]
    grpc: Capability,
    #[serde(default)]
    evolution: Capability,
    #[serde(default)]
    identity: Resource,
    #[serde(default)]
    database: Resource,
}

impl Default for Composition {
    fn default() -> Self {
        Self {
            schema_version: 1,
            rhai: Capability::default(),
            typescript: Capability::default(),
            mcp: Capability::default(),
            grpc: Capability::default(),
            evolution: Capability::default(),
            identity: Resource::default(),
            database: Resource::default(),
        }
    }
}

/// 产品拥有部署文件及已提交组合；插件身份始终由具体类型决定，不序列化 TypeId。
pub struct RuntimeComposition {
    path: Option<PathBuf>,
    applied: Composition,
    plugins: Vec<DynPlugin<RuntimeAssembly>>,
    identity: Arc<aio_plugin_identity_server::IdentityService>,
    observed: Vec<u8>,
}

impl RuntimeComposition {
    pub async fn load(
        plugins: Vec<DynPlugin<RuntimeAssembly>>,
        identity: Arc<aio_plugin_identity_server::IdentityService>,
    ) -> Result<Self> {
        let path = std::env::var_os("AIO_RUNTIME_COMPOSITION").map(PathBuf::from);
        Self::open(path, plugins, identity).await
    }

    async fn open(
        path: Option<PathBuf>,
        plugins: Vec<DynPlugin<RuntimeAssembly>>,
        identity: Arc<aio_plugin_identity_server::IdentityService>,
    ) -> Result<Self> {
        let mut control = Self {
            path,
            plugins,
            identity,
            applied: Composition::default(),
            observed: Vec::new(),
        };
        if let Some(path) = &control.path {
            let bytes = read(path).await?;
            let selected = parse(&bytes)?;
            control.plugins = control.select(&selected);
            control.applied = selected;
            control.observed = bytes;
        }
        Ok(control)
    }

    pub fn plugins(&self) -> Vec<DynPlugin<RuntimeAssembly>> {
        self.plugins.clone()
    }

    fn select(&self, next: &Composition) -> Vec<DynPlugin<RuntimeAssembly>> {
        let mut plugins = self.plugins.clone();
        select(&mut plugins, &self.applied.rhai, &next.rhai, RhaiPlugin);
        select(
            &mut plugins,
            &self.applied.typescript,
            &next.typescript,
            TypeScriptPlugin,
        );
        select(&mut plugins, &self.applied.mcp, &next.mcp, McpPlugin);
        select(&mut plugins, &self.applied.grpc, &next.grpc, GrpcPlugin);
        select(
            &mut plugins,
            &self.applied.evolution,
            &next.evolution,
            EvolutionPlugin,
        );
        if self.applied.identity != next.identity {
            replace(&mut plugins, ProductIdentityPlugin(self.identity.clone()));
        }
        if self.applied.database != next.database {
            replace(&mut plugins, DatabasePlugin);
        }
        plugins
    }

    // 相同失败内容只尝试一次；新文件会重新校验，成功之前保留已提交的配方。
    async fn apply(&mut self, host: &RuntimeHost, bytes: Vec<u8>) -> Result<()> {
        if bytes == self.observed {
            return Ok(());
        }
        self.observed = bytes;
        let selected = parse(&self.observed)?;
        if selected == self.applied {
            return Ok(());
        }
        let plugins = self.select(&selected);
        host.reconfigure(plugins.clone()).await?;
        self.plugins = plugins;
        self.applied = selected;
        eprintln!("已应用部署方运行时组合: {:?}", self.applied);
        Ok(())
    }

    pub async fn watch(mut self, host: RuntimeHost) -> Result<()> {
        let Some(path) = self.path.clone() else {
            return std::future::pending().await;
        };
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut reported = None;
        loop {
            interval.tick().await;
            let result = match read(&path).await {
                Ok(bytes) => self.apply(&host, bytes).await,
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                // 恢复失败时交回进程监督器，不能继续报告正常健康状态。
                if !host.is_available() {
                    return Err(error.context("运行时组合恢复失败，停止监听并交由监督器处理"));
                }
                let detail = format!("{error:#}");
                if reported.as_ref() != Some(&detail) {
                    eprintln!("运行时组合未应用，保留旧版: {detail}");
                    reported = Some(detail);
                }
            } else {
                reported = None;
            }
        }
    }
}

fn select<P: Plugin<RuntimeAssembly>>(
    plugins: &mut Vec<DynPlugin<RuntimeAssembly>>,
    previous: &Capability,
    next: &Capability,
    plugin: P,
) {
    if previous == next {
        return;
    }
    if next.enabled {
        replace(plugins, plugin);
    } else {
        plugins.retain(|entry| entry.as_ref().type_id() != TypeId::of::<P>());
    }
}

fn replace<P: Plugin<RuntimeAssembly>>(plugins: &mut Vec<DynPlugin<RuntimeAssembly>>, plugin: P) {
    let plugin: DynPlugin<RuntimeAssembly> = Arc::new(plugin);
    if let Some(previous) = plugins
        .iter_mut()
        .find(|entry| entry.as_ref().type_id() == TypeId::of::<P>())
    {
        *previous = plugin;
    } else {
        plugins.push(plugin);
    }
}

fn parse(bytes: &[u8]) -> Result<Composition> {
    let selected: Composition = serde_json::from_slice(bytes).context("解析原生运行时组合失败")?;
    ensure!(
        selected.schema_version == 1,
        "运行时组合 schema_version 必须为 1"
    );
    Ok(selected)
}

async fn read(path: &Path) -> Result<Vec<u8>> {
    let metadata = tokio::fs::metadata(path)
        .await
        .context("读取运行时组合文件属性失败")?;
    ensure!(metadata.is_file(), "运行时组合必须是普通文件");
    let file = tokio::fs::File::open(path)
        .await
        .context("打开运行时组合文件失败")?;
    let mut bytes = Vec::new();
    file.take(MAX_COMPOSITION_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .context("读取运行时组合文件失败")?;
    ensure!(
        bytes.len() as u64 <= MAX_COMPOSITION_BYTES,
        "运行时组合文件不能超过 64 KiB"
    );
    Ok(bytes)
}

#[cfg(test)]
#[path = "runtime_composition_tests.rs"]
mod tests;
