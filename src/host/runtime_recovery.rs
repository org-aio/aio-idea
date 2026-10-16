//! 本机恢复命令直接使用启动记录，不依赖业务身份、数据库和插件是否可用。

use std::path::Path;

use super::*;

pub(super) fn source_path() -> Option<PathBuf> {
    std::env::var_os("AIO_RUNTIME_COMPOSITION").map(PathBuf::from)
}

/// 正常宿主和命令共用同一来源与缓存目录；status 只返回记录快照。
pub fn runtime_command(command: &str) -> Result<()> {
    let source = source_path().context("恢复命令需要 AIO_RUNTIME_COMPOSITION")?;
    let cache_root = crate::host::configuration::cache_root();
    let output = execute(command, &source, &cache_root)?;
    println!("{output}");
    Ok(())
}

pub(super) fn execute(command: &str, source: &Path, cache_root: &Path) -> Result<String> {
    let directory = cache_root.join("runtime-composition");
    match command {
        "status" => {
            let record = ConfigurationJournal::inspect(&directory, source)?;
            Ok(serde_json::to_string_pretty(&record)?)
        }
        "migrate" => {
            let mut journal = ConfigurationJournal::open(&directory, source)?;
            let selected = migrate_saved(&read_configuration(source)?)?;
            journal.stage_configuration(&serde_json::to_value(selected)?)?;
            Ok("已迁移部署候选；字典启停由已安装整包管理，下次实际启动才确认新组合".into())
        }
        "retry" => {
            let mut journal = ConfigurationJournal::open(&directory, source)?;
            parse(&read_configuration(source)?)?;
            journal.allow_retry()?;
            Ok("已允许下次启动重试部署配置；运行时初始化仍会重新校验".into())
        }
        "restore" => {
            let mut journal = ConfigurationJournal::open(&directory, source)?;
            let confirmed = journal.confirmed().context("没有已确认的可恢复配置")?;
            let selected = migrate_saved(&serde_json::to_vec(&confirmed.value)?)?;
            journal.stage_configuration(&serde_json::to_value(selected)?)?;
            Ok("已恢复最后成功组合；可以重新启动宿主".into())
        }
        _ => anyhow::bail!("用法: aio-idea runtime status|retry|restore|migrate"),
    }
}
