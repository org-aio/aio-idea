//! 部署方声明原生能力组合；文件更新由监听器所有者处理，不进入待排空的插件任务。

use std::{any::TypeId, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use az_plugin_core::{DynPlugin, Plugin};
use az_plugin_host::recovery::{ConfigurationJournal, configuration_digest, read_configuration};
use az_plugin_host::runtime::server::plugins::{
    ComponentManagementPlugin, DatabasePlugin, EnhancementServicePlugin, EnhancementsPlugin,
    EvolutionPlugin, GrpcPlugin, McpPlugin, PostgresEnhancementCatalogPlugin, ProcessPlugin,
    RhaiPlugin, RuntimeAssembly, RuntimeHost, ScriptRuntimePlugin, SidecarPlugin,
    TrustedManagementComponent, TypeScriptPlugin, WasmComponentPlugin, WasmPlugin,
};
use serde::{Deserialize, Serialize};

use crate::plugins::server::IdentityServicePlugin;

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
    wasm: Capability,
    #[serde(default)]
    sidecar: Capability,
    #[serde(default)]
    mcp: Capability,
    #[serde(default)]
    grpc: Capability,
    #[serde(default)]
    evolution: Capability,
    #[serde(default)]
    device_tunnels: Capability,
    #[serde(default)]
    trusted_management_components: Vec<TrustedManagementComponent>,
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
            wasm: Capability::default(),
            sidecar: Capability::default(),
            mcp: Capability::default(),
            grpc: Capability::default(),
            evolution: Capability::default(),
            device_tunnels: Capability::default(),
            trusted_management_components: Vec::new(),
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
                let plugins = select_plugins(&self.plugins, &self.applied, &selected);
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
    // 同一运行环境同时管理两代插件契约，避免只暂停其中一种入口。
    select(&mut plugins, &previous.wasm, &next.wasm, WasmPlugin);
    select(
        &mut plugins,
        &previous.wasm,
        &next.wasm,
        WasmComponentPlugin,
    );
    select(
        &mut plugins,
        &previous.sidecar,
        &next.sidecar,
        ProcessPlugin,
    );
    select(
        &mut plugins,
        &previous.sidecar,
        &next.sidecar,
        SidecarPlugin,
    );
    // 脚本服务随可用语言装配；语言重启引发的依赖重建仍由运行时处理。
    let previous_scripts = Capability {
        enabled: previous.rhai.enabled || previous.typescript.enabled,
        restart: 0,
    };
    let next_scripts = Capability {
        enabled: next.rhai.enabled || next.typescript.enabled,
        restart: 0,
    };
    select(
        &mut plugins,
        &previous_scripts,
        &next_scripts,
        ScriptRuntimePlugin,
    );
    select(
        &mut plugins,
        &previous_scripts,
        &next_scripts,
        PostgresEnhancementCatalogPlugin,
    );
    select(
        &mut plugins,
        &previous_scripts,
        &next_scripts,
        EnhancementServicePlugin,
    );
    select(
        &mut plugins,
        &previous_scripts,
        &next_scripts,
        EnhancementsPlugin,
    );
    // 保留部署方的演进开关；语言全部停用时暂停，语言恢复后再按原开关装配。
    let previous_evolution = Capability {
        enabled: previous.evolution.enabled && previous_scripts.enabled,
        ..previous.evolution.clone()
    };
    let next_evolution = Capability {
        enabled: next.evolution.enabled && next_scripts.enabled,
        ..next.evolution.clone()
    };
    select(
        &mut plugins,
        &previous_evolution,
        &next_evolution,
        EvolutionPlugin,
    );
    select(
        &mut plugins,
        &previous.device_tunnels,
        &next.device_tunnels,
        aio_plugin_device_server::DeviceTunnelPlugin,
    );
    // 信任与首次引导分开持久化，移除摘要会重建组件环境并暂停该版本的高权限能力。
    if previous.trusted_management_components != next.trusted_management_components {
        if next.trusted_management_components.is_empty() {
            plugins.retain(|entry| {
                entry.as_ref().type_id() != TypeId::of::<ComponentManagementPlugin>()
            });
        } else {
            replace(
                &mut plugins,
                ComponentManagementPlugin(next.trusted_management_components.clone()),
            );
        }
    }
    if previous.identity != next.identity {
        replace(&mut plugins, IdentityServicePlugin);
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

// 只迁移持久化数据；新部署输入拒绝已退出原生组合的系统整包开关。
fn migrate_saved(bytes: &[u8]) -> Result<Composition> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    for name in [
        "dictionary",
        "marketplace",
        "files",
        "tenants",
        "access_control",
    ] {
        if let Some(retired) = value.as_object_mut().and_then(|v| v.remove(name)) {
            let _: Capability = serde_json::from_value(retired)
                .with_context(|| format!("旧系统插件开关格式无效: {name}"))?;
        }
    }
    parse(&serde_json::to_vec(&value)?)
}

#[cfg(test)]
#[path = "runtime_composition_tests.rs"]
mod tests;
