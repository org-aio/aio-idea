use std::fs;

use serde_json::json;

use super::*;
use crate::host::runtime_composition::recovery::execute;

#[test]
fn retired_system_settings_migrate_without_reenabling_other_services() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("composition.json");
    let cache = root.path().join("cache");
    let old = json!({"schema_version":1,"dictionary":{"enabled":false,"restart":3},"marketplace":{"enabled":false,"restart":2},"files":{"enabled":false},"tenants":{"enabled":false,"restart":1},"access_control":{"enabled":false},"mcp":{"enabled":false}});
    let bytes = serde_json::to_vec(&old)?;
    fs::write(&source, &bytes)?;
    let mut journal = ConfigurationJournal::open(&cache.join("runtime-composition"), &source)?;
    let attempt = journal.begin(configuration_digest(&bytes))?;
    journal.confirm(&attempt, old.clone())?;
    drop(journal);
    assert!(parse(&bytes).is_err(), "新输入不能继续使用退役系统开关");
    assert!(parse(br#"{"schema_version":1,"marketplace":{"enabled":true}}"#).is_err());
    assert!(migrate_saved(br#"{"schema_version":1,"marketplace":{"enabled":"invalid"}}"#).is_err());
    let recovered = RuntimeBootstrap::open(Some(source.clone()), &cache)?;
    assert!(!recovered.selected.access_control.enabled);
    assert!(!recovered.selected.mcp.enabled);
    assert!(
        execute("migrate", &source, &cache).is_err(),
        "运行时持锁时不能改配置"
    );
    drop(recovered);
    execute("migrate", &source, &cache)?;
    let selected = parse(&read_configuration(&source)?)?;
    assert!(!selected.access_control.enabled);
    assert!(!selected.mcp.enabled);
    let record = ConfigurationJournal::inspect(&cache.join("runtime-composition"), &source)?
        .context("缺少恢复记录")?;
    assert_eq!(record.confirmed.context("缺少成功组合")?.value, old);
    let mut restarted = RuntimeBootstrap::open(Some(source), &cache)?;
    assert!(restarted.attempt.is_some());
    confirm(&mut restarted)?;
    let confirmed = restarted
        .journal
        .as_ref()
        .context("缺少日志")?
        .confirmed()
        .context("缺少成功组合")?
        .value
        .clone();
    assert!(confirmed.get("dictionary").is_none());
    assert!(confirmed.get("marketplace").is_none());
    assert!(confirmed.get("files").is_none());
    assert!(parse(br#"{"schema_version":1,"files":{"enabled":true}}"#).is_err());
    assert!(migrate_saved(br#"{"schema_version":1,"files":{"enabled":"invalid"}}"#).is_err());
    assert!(confirmed.get("tenants").is_none(), "租户开关随静态组合退出");
    assert!(parse(br#"{"schema_version":1,"tenants":{"enabled":true}}"#).is_err());
    assert!(migrate_saved(br#"{"schema_version":1,"tenants":{"enabled":"invalid"}}"#).is_err());
    Ok(())
}

fn confirm(bootstrap: &mut RuntimeBootstrap) -> Result<()> {
    let journal = bootstrap.journal.as_mut().context("缺少恢复记录")?;
    let attempt = bootstrap.attempt.as_ref().context("缺少启动尝试")?;
    journal.confirm(attempt, serde_json::to_value(&bootstrap.selected)?)
}

#[test]
fn invalid_and_interrupted_candidates_preserve_disabled_capabilities_across_restart() -> Result<()>
{
    let root = tempfile::tempdir()?;
    let source = root.path().join("composition.json");
    let cache = root.path().join("cache");
    fs::write(
        &source,
        br#"{"schema_version":1,"mcp":{"enabled":false},"evolution":{"enabled":false}}"#,
    )?;
    let mut initial = RuntimeBootstrap::open(Some(source.clone()), &cache)?;
    let confirmed = initial.selected.clone();
    confirm(&mut initial)?;
    drop(initial);

    for bytes in [
        br#"{"schema_version":2}"#.as_slice(),
        br#"{"schema_version":1,"mcp":{"enabled":true}}"#.as_slice(),
    ] {
        fs::write(&source, bytes)?;
        let candidate = RuntimeBootstrap::open(Some(source.clone()), &cache)?;
        drop(candidate);
        let recovered = RuntimeBootstrap::open(Some(source.clone()), &cache)?;
        assert_eq!(recovered.selected, confirmed);
        assert!(recovered.attempt.is_none());
        let record = execute("status", &source, &cache)?;
        assert!(record.contains(&configuration_digest(bytes)));
        assert!(execute("retry", &source, &cache).is_err());
        assert!(execute("restore", &source, &cache).is_err());
        drop(recovered);
    }
    execute("retry", &source, &cache)?;
    let retried = RuntimeBootstrap::open(Some(source.clone()), &cache)?;
    assert!(retried.attempt.is_some());
    assert!(retried.selected.mcp.enabled);
    drop(retried);
    execute("restore", &source, &cache)?;
    assert_eq!(parse(&read_configuration(&source)?)?, confirmed);
    let restored = RuntimeBootstrap::open(Some(source), &cache)?;
    assert_eq!(restored.selected, confirmed);
    Ok(())
}

#[test]
fn missing_source_uses_confirmed_configuration_and_first_start_fails_closed() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("composition.json");
    let cache = root.path().join("cache");
    assert_eq!(execute("status", &source, &cache)?, "null");
    assert!(!cache.exists());
    assert!(RuntimeBootstrap::open(Some(source.clone()), &cache).is_err());
    fs::write(&source, br#"{"schema_version":1,"grpc":{"enabled":false}}"#)?;
    let mut initial = RuntimeBootstrap::open(Some(source.clone()), &cache)?;
    confirm(&mut initial)?;
    drop(initial);
    fs::remove_file(&source)?;
    let recovered = RuntimeBootstrap::open(Some(source.clone()), &cache)?;
    assert!(!recovered.selected.grpc.enabled);
    assert!(recovered.attempt.is_none());
    drop(recovered);
    execute("restore", &source, &cache)?;
    assert!(!parse(&read_configuration(&source)?)?.grpc.enabled);
    Ok(())
}

#[test]
fn recovery_revalidates_current_contract_and_rejects_corrupt_or_foreign_journals() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("composition.json");
    let cache = root.path().join("cache");
    let directory = cache.join("runtime-composition");
    let mut journal = ConfigurationJournal::open(&directory, &source)?;
    let attempt = journal.begin(configuration_digest(b"old contract"))?;
    journal.confirm(&attempt, json!({"schema_version":7}))?;
    drop(journal);
    assert!(RuntimeBootstrap::open(Some(source.clone()), &cache).is_err());
    assert!(execute("restore", &source, &cache).is_err());
    assert!(!source.exists());
    assert!(execute("status", &root.path().join("foreign.json"), &cache).is_err());
    fs::write(directory.join("record.json"), b"{")?;
    fs::write(&source, br#"{"schema_version":1}"#)?;
    assert!(RuntimeBootstrap::open(Some(source.clone()), &cache).is_err());
    assert!(execute("retry", &source, &cache).is_err());
    Ok(())
}
