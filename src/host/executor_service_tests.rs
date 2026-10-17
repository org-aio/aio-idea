//! 产品的执行器开关同时覆盖两代契约，协议入口在暂停期间保持可用。

use super::*;
use az_plugin_host::runtime::server::{
    RuntimeState,
    plugins::{ProcessExecutor, WasmExecutor},
};
use std::sync::Mutex;

pub(super) async fn verify(
    control: &mut RuntimeComposition,
    host: &RuntimeHost,
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    generations: &Mutex<Vec<RuntimeState>>,
) -> Result<()> {
    let before = system_services::latest(generations)?;
    let wasm = before.service::<dyn WasmExecutor>()?;
    let process = before.service::<dyn ProcessExecutor>()?;
    apply(
        control,
        host,
        json!({"schema_version":1,"wasm":{"enabled":false},"sidecar":{"enabled":false}}),
    )
    .await?;
    let paused = system_services::latest(generations)?;
    assert!(paused.optional_service::<dyn WasmExecutor>()?.is_none());
    assert!(paused.optional_service::<dyn ProcessExecutor>()?.is_none());
    assert_eq!(mcp(client, base, cookie).await?, 200);
    assert_eq!(grpc(client, base, cookie).await?, tonic::Code::Ok);
    assert_snapshot(client, base, cookie).await?;
    apply(
        control,
        host,
        json!({"schema_version":1,"sidecar":{"enabled":false}}),
    )
    .await?;
    let partial = system_services::latest(generations)?;
    assert!(!Arc::ptr_eq(&wasm, &partial.service::<dyn WasmExecutor>()?));
    assert!(partial.optional_service::<dyn ProcessExecutor>()?.is_none());
    apply(
        control,
        host,
        json!({"schema_version":1,"wasm":{"restart":1},"sidecar":{"restart":1}}),
    )
    .await?;
    let restored = system_services::latest(generations)?;
    assert!(!Arc::ptr_eq(
        &process,
        &restored.service::<dyn ProcessExecutor>()?
    ));
    assert!(!Arc::ptr_eq(
        &partial.service::<dyn WasmExecutor>()?,
        &restored.service::<dyn WasmExecutor>()?
    ));
    Ok(())
}
