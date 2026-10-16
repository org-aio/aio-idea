//! 部署方声明原生能力组合；文件更新由监听器所有者处理，不进入待排空的插件任务。

use std::{any::TypeId, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use az_plugin_core::{DynPlugin, Plugin};
use az_plugin_host::recovery::{ConfigurationJournal, configuration_digest, read_configuration};
use az_plugin_host::runtime::server::plugins::{
    DatabasePlugin, EvolutionPlugin, GrpcPlugin, McpPlugin, RhaiPlugin, RuntimeAssembly,
    RuntimeHost, TypeScriptPlugin,
};
use serde::{Deserialize, Serialize};

use super::ProductIdentityPlugin;

#[path = "runtime_bootstrap.rs"]
mod bootstrap;
#[path = "runtime_recovery.rs"]
mod recovery;
pub use bootstrap::RuntimeBootstrap;
pub use recovery::runtime_command;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
struct Resource {
    restart: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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
    observed: Option<String>,
    journal: Option<ConfigurationJournal>,
}

impl RuntimeComposition {
    // 候选执行前先落盘，崩溃或失败后相同内容不会在下次启动自动重试。
    async fn apply(&mut self, host: &RuntimeHost, bytes: Vec<u8>) -> Result<()> {
        let digest = configuration_digest(&bytes);
        if self.observed.as_ref() == Some(&digest) {
            return Ok(());
        }
        let journal = self.journal.as_mut().context("缺少部署恢复记录")?;
        let attempt = journal.begin(digest.clone())?;
        self.observed = Some(digest);
        let result = async {
            let selected = parse(&bytes)?;
            if selected != self.applied {
                let plugins =
                    select_plugins(&self.plugins, &self.identity, &self.applied, &selected);
                host.reconfigure(plugins.clone()).await?;
                self.plugins = plugins;
                self.applied = selected;
            }
            anyhow::Ok(())
        }
        .await;
        if let Err(error) = result {
            return match journal.reject(&attempt) {
                Ok(()) => Err(error),
                Err(rejection) => Err(error.context(format!("保存拒绝记录失败: {rejection:#}"))),
            };
        }
        journal.confirm(&attempt, serde_json::to_value(&self.applied)?)?;
        eprintln!("已应用并确认部署方运行时组合: {:?}", self.applied);
        Ok(())
    }

    pub async fn watch(&mut self, host: &RuntimeHost) -> Result<()> {
        let Some(path) = self.path.clone() else {
            return std::future::pending().await;
        };
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut reported = None;
        loop {
            interval.tick().await;
            let result = match read_configuration(&path) {
                Ok(bytes) => self.apply(host, bytes).await,
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                // 恢复失败时交回进程监督器，不能继续报告正常健康状态。
                if !host.is_available()
                    || self
                        .journal
                        .as_ref()
                        .is_some_and(|journal| !journal.is_healthy())
                {
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

fn select_plugins(
    current: &[DynPlugin<RuntimeAssembly>],
    identity: &Arc<aio_plugin_identity_server::IdentityService>,
    previous: &Composition,
    next: &Composition,
) -> Vec<DynPlugin<RuntimeAssembly>> {
    let mut plugins = current.to_vec();
    select(&mut plugins, &previous.rhai, &next.rhai, RhaiPlugin);
    select(
        &mut plugins,
        &previous.typescript,
        &next.typescript,
        TypeScriptPlugin,
    );
    select(&mut plugins, &previous.mcp, &next.mcp, McpPlugin);
    select(&mut plugins, &previous.grpc, &next.grpc, GrpcPlugin);
    select(
        &mut plugins,
        &previous.evolution,
        &next.evolution,
        EvolutionPlugin,
    );
    if previous.identity != next.identity {
        replace(&mut plugins, ProductIdentityPlugin(identity.clone()));
    }
    if previous.database != next.database {
        replace(&mut plugins, DatabasePlugin);
    }
    plugins
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

#[cfg(test)]
#[path = "runtime_composition_tests.rs"]
mod tests;
