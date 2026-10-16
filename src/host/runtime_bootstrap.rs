//! 在业务服务和插件初始化前选择可恢复的部署组合，持锁直到运行时停止。

use std::path::Path;

use axum::Router;
use az_plugin_host::{
    configuration::HostConfig, recovery::ConfigurationAttempt, runtime::server::RuntimeState,
};

use super::*;

/// 只准备配置，不连接数据库；未完成的启动记录由下一个新进程隔离。
pub struct RuntimeBootstrap {
    path: Option<PathBuf>,
    journal: Option<ConfigurationJournal>,
    selected: Composition,
    observed: Option<String>,
    attempt: Option<ConfigurationAttempt>,
}

impl RuntimeBootstrap {
    pub fn load(cache_root: &Path) -> Result<Self> {
        Self::open(super::recovery::source_path(), cache_root)
    }

    pub(super) fn open(path: Option<PathBuf>, cache_root: &Path) -> Result<Self> {
        let mut bootstrap = Self {
            path,
            journal: None,
            selected: Composition::default(),
            observed: None,
            attempt: None,
        };
        let Some(path) = bootstrap.path.clone() else {
            return Ok(bootstrap);
        };
        let mut journal =
            ConfigurationJournal::open(&cache_root.join("runtime-composition"), &path)?;
        let candidate = bootstrap.prepare(&mut journal, &path);
        bootstrap.selected = match candidate {
            Ok(selected) => selected,
            Err(error) => {
                // 持久化失败必须直接退出，不能掩盖为正常回退。
                if !journal.is_healthy() {
                    return Err(error);
                }
                let confirmed = journal.confirmed().with_context(|| {
                    format!("部署配置不可用且没有已确认组合；请修正部署文件: {error:#}")
                })?;
                let selected = parse(&serde_json::to_vec(&confirmed.value)?)
                    .context("最后成功组合不符合当前产品契约，拒绝启动")?;
                eprintln!("部署配置未应用，启动最后成功组合: {error:#}");
                selected
            }
        };
        bootstrap.journal = Some(journal);
        Ok(bootstrap)
    }

    fn prepare(&mut self, journal: &mut ConfigurationJournal, path: &Path) -> Result<Composition> {
        let bytes = read_configuration(path)?;
        let digest = configuration_digest(&bytes);
        self.observed = Some(digest.clone());
        let attempt = journal.begin(digest)?;
        let selected = match parse(&bytes) {
            Ok(selected) => selected,
            Err(error) => {
                journal
                    .reject(&attempt)
                    .context(format!("部署配置无效: {error:#}"))?;
                return Err(error);
            }
        };
        self.attempt = Some(attempt);
        Ok(selected)
    }

    /// 调用方先绑定监听器并完成业务初始化；只有资源和完整应用路由就绪才确认。
    pub async fn start<F>(
        mut self,
        config: HostConfig,
        plugins: Vec<DynPlugin<RuntimeAssembly>>,
        identity: Arc<aio_plugin_identity_server::IdentityService>,
        compose: F,
    ) -> Result<(RuntimeHost, RuntimeComposition)>
    where
        F: Fn(&RuntimeState, Router) -> Result<Router> + Send + Sync + 'static,
    {
        let plugins = select_plugins(&plugins, &identity, &Composition::default(), &self.selected);
        let initialized =
            RuntimeHost::initialize_with_router(config, plugins.clone(), compose).await;
        let host = match initialized {
            Ok(host) => host,
            Err(error) => {
                if let (Some(journal), Some(attempt)) = (&mut self.journal, &self.attempt) {
                    journal
                        .reject(attempt)
                        .context(format!("运行时启动失败: {error:#}"))?;
                }
                // 初始化可能留下无法清理的资源，只允许下一个新进程加载最后成功组合。
                return Err(error.context("运行时启动失败，停止本次启动"));
            }
        };
        if let (Some(journal), Some(attempt)) = (&mut self.journal, &self.attempt) {
            let confirmed = journal.confirm(attempt, serde_json::to_value(&self.selected)?);
            if let Err(error) = confirmed {
                return match host.shutdown().await {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(error.context(format!("运行时停止失败: {cleanup:#}"))),
                };
            }
        }
        let control = RuntimeComposition {
            path: self.path,
            journal: self.journal,
            applied: self.selected,
            observed: self.observed,
            plugins,
            identity,
        };
        Ok((host, control))
    }
}

#[cfg(test)]
#[path = "runtime_recovery_tests.rs"]
mod tests;
