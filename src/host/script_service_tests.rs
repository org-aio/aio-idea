//! 使用真实产品身份、增强版本及设备能力验证零语言、单语言和重新装配。

use std::sync::Mutex;

use aio_plugin_identity_server::IdentityService;
use az_plugin_contract::grpc::{
    InvokeRequest, ListRequest, capabilities_client::CapabilitiesClient,
    enhancements_client::EnhancementsClient,
};
use az_plugin_host::runtime::server::{
    RuntimeState,
    plugins::{RuntimeCapabilities, ScriptRuntime},
};
use tonic::{Code, Request, transport::Channel};

use super::*;

const PATH: &str = "/v1/product/bootstrap";
const TOOL: &str = "v1.product.bootstrap";
const NATIVE: &str = "/v1/workers/list";

pub(super) async fn verify(
    control: &mut RuntimeComposition,
    host: &RuntimeHost,
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    generations: &Mutex<Vec<RuntimeState>>,
) -> Result<()> {
    let initial = system_services::latest(generations)?;
    let pool = initial.service::<sqlx::PgPool>()?;
    // 仅为独立测试库的管理员配置一项显式能力；产品管理权限不能替代 runtime 授权。
    sqlx::query("INSERT INTO role_permissions (tenant_id,role_id,permission) VALUES ('default','platform-admin',$1) ON CONFLICT DO NOTHING")
        .bind(format!("runtime:{NATIVE}")).execute(pool.as_ref()).await?;
    let channel = tonic::transport::Endpoint::from_shared(base.to_owned())?
        .timeout(Duration::from_secs(10))
        .connect()
        .await?;
    let mut native = CapabilitiesClient::new(channel.clone());
    let mut enhancements = EnhancementsClient::new(channel);
    assert_native(client, base, cookie, &mut native).await?;

    for (language, other) in [("rhai", "typescript"), ("typescript", "rhai")] {
        let revision = publish(client, base, cookie, package(language)?).await?;
        let saved_head = head(client, base, cookie).await?;
        assert_script(client, base, cookie, &mut enhancements, &revision).await?;
        let before = system_services::latest(generations)?;
        assert_evolution(&before, true)?;

        apply(control, host, no_languages()).await?;
        let without = system_services::latest(generations)?;
        assert!(without.service::<ScriptRuntime>().is_err());
        assert!(Arc::ptr_eq(&pool, &without.service::<sqlx::PgPool>()?));
        assert!(Arc::ptr_eq(
            &initial.service::<IdentityService>()?,
            &without.service::<IdentityService>()?
        ));
        assert_evolution(&without, false)?;
        for path in ["", "/revisions"] {
            assert_eq!(
                client
                    .get(format!("{base}/api/runtime/enhancements{path}"))
                    .header("Cookie", cookie)
                    .send()
                    .await?
                    .status(),
                404
            );
        }
        let unavailable = enhancements
            .list(auth(ListRequest {}, cookie)?)
            .await
            .err()
            .context("增强服务未被卸载")?;
        assert_eq!(unavailable.code(), Code::Unimplemented, "{unavailable:?}");
        let tools = mcp_request(client, base, cookie, "tools/list", json!({})).await?;
        assert_eq!(tools["tools"].as_array().context("缺少目录")?.len(), 1);
        assert_eq!(tools["tools"][0]["name"], "runtime.v1.workers.list");
        assert_native(client, base, cookie, &mut native).await?;

        // 仅恢复另一种语言：活动版本保留，不能阻塞原生能力的发现与调用。
        let mut mismatched = no_languages();
        mismatched[other]["enabled"] = json!(true);
        apply(control, host, mismatched).await?;
        assert_eq!(head(client, base, cookie).await?, saved_head);
        let tools = mcp_request(client, base, cookie, "tools/list", json!({})).await?;
        assert_eq!(tools["tools"].as_array().context("缺少目录")?.len(), 1);
        assert_eq!(tools["tools"][0]["name"], "runtime.v1.workers.list");
        let catalog = enhancements
            .list(auth(ListRequest {}, cookie)?)
            .await?
            .into_inner();
        assert_eq!(catalog.revision.as_deref(), Some(revision.as_str()));
        assert!(catalog.paths.is_empty());
        let unavailable = enhancements
            .invoke(auth(
                InvokeRequest {
                    path: PATH.into(),
                    input_json: b"21".to_vec(),
                },
                cookie,
            )?)
            .await
            .err()
            .context("停用语言仍能执行增强")?;
        assert_eq!(
            unavailable.code(),
            Code::PermissionDenied,
            "{unavailable:?}"
        );
        assert_native(client, base, cookie, &mut native).await?;

        // 保持同一 HTTP 客户端和 gRPC 连接，仅恢复一种语言及其依赖服务。
        let mut selected = no_languages();
        selected[language]["enabled"] = json!(true);
        apply(control, host, selected.clone()).await?;
        let restored = system_services::latest(generations)?;
        assert!(!Arc::ptr_eq(
            &before.service::<ScriptRuntime>()?,
            &restored.service::<ScriptRuntime>()?
        ));
        assert_evolution(&restored, true)?;
        assert_eq!(head(client, base, cookie).await?, saved_head);
        assert_script(client, base, cookie, &mut enhancements, &revision).await?;
        assert_native(client, base, cookie, &mut native).await?;
        assert!(
            client
                .post(format!("{base}/api/runtime/enhancements/revisions"))
                .header("Cookie", cookie)
                .json(&package(other)?)
                .send()
                .await?
                .error_for_status()
                .is_err(),
            "停用语言仍能编译候选"
        );
        assert_eq!(head(client, base, cookie).await?, saved_head);

        selected[language]["restart"] = json!(1);
        apply(control, host, selected).await?;
        assert!(!Arc::ptr_eq(
            &restored.service::<ScriptRuntime>()?,
            &system_services::latest(generations)?.service::<ScriptRuntime>()?
        ));
        assert_script(client, base, cookie, &mut enhancements, &revision).await?;
        assert_eq!(head(client, base, cookie).await?, saved_head);
        apply(control, host, json!({"schema_version":1})).await?;
    }

    // 没有语言期间关闭演进能力；恢复语言不得覆盖这个明确的停用意图。
    let mut selected = no_languages();
    selected["evolution"] = json!({"enabled":false,"restart":1});
    apply(control, host, selected).await?;
    apply(
        control,
        host,
        json!({"schema_version":1,"evolution":{"enabled":false,"restart":1}}),
    )
    .await?;
    assert_evolution(&system_services::latest(generations)?, false)?;
    let current = head(client, base, cookie).await?;
    let revision = current["revision"].as_str().context("增强版本丢失")?;
    assert_script(client, base, cookie, &mut enhancements, revision).await?;
    assert_native(client, base, cookie, &mut native).await?;
    apply(control, host, json!({"schema_version":1})).await?;
    assert_evolution(&system_services::latest(generations)?, true)?;
    client
        .put(format!("{base}/api/runtime/enhancements/active"))
        .header("Cookie", cookie)
        .json(&json!({"revision":null,"expected_generation":current["generation"]}))
        .send()
        .await?
        .error_for_status()?;
    sqlx::query("DELETE FROM role_permissions WHERE tenant_id='default' AND role_id='platform-admin' AND permission=$1")
        .bind(format!("runtime:{NATIVE}")).execute(pool.as_ref()).await?;
    assert!(
        native
            .list(auth(ListRequest {}, cookie)?)
            .await?
            .into_inner()
            .paths
            .is_empty()
    );
    Ok(())
}

fn no_languages() -> Value {
    json!({"schema_version":1,"rhai":{"enabled":false},"typescript":{"enabled":false}})
}

fn assert_evolution(state: &RuntimeState, enabled: bool) -> Result<()> {
    let catalog = state.service::<RuntimeCapabilities>()?.catalog("default")?;
    assert_eq!(
        catalog
            .iter()
            .any(|capability| capability.path.starts_with("/v1/runtime/enhancements/")),
        enabled
    );
    Ok(())
}

fn auth<T>(value: T, cookie: &str) -> Result<Request<T>> {
    let mut request = Request::new(value);
    request.metadata_mut().insert("cookie", cookie.parse()?);
    Ok(request)
}

async fn mcp_request(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let response = client
        .post(format!("{base}/api/runtime/mcp"))
        .header("Cookie", cookie)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-06-18")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    assert!(response.get("error").is_none(), "{response}");
    Ok(response["result"].clone())
}

async fn assert_native(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    grpc: &mut CapabilitiesClient<Channel>,
) -> Result<()> {
    let result = mcp_request(
        client,
        base,
        cookie,
        "tools/call",
        json!({"name":"runtime.v1.workers.list","arguments":{"input":{}}}),
    )
    .await?;
    assert_ne!(result["isError"], true, "{result}");
    let output = &result["structuredContent"]["output"];
    assert!(output.is_array());
    let response = grpc
        .invoke(auth(
            InvokeRequest {
                path: NATIVE.into(),
                input_json: b"{}".to_vec(),
            },
            cookie,
        )?)
        .await?
        .into_inner();
    assert_eq!(
        &serde_json::from_slice::<Value>(&response.output_json)?,
        output
    );
    Ok(())
}

async fn assert_script(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    grpc: &mut EnhancementsClient<Channel>,
    revision: &str,
) -> Result<()> {
    let result = mcp_request(
        client,
        base,
        cookie,
        "tools/call",
        json!({"name":TOOL,"arguments":{"input":21}}),
    )
    .await?;
    assert_ne!(result["isError"], true, "{result}");
    assert_eq!(
        result["structuredContent"],
        json!({"revision":revision,"output":42})
    );
    let response = grpc
        .invoke(auth(
            InvokeRequest {
                path: PATH.into(),
                input_json: b"21".to_vec(),
            },
            cookie,
        )?)
        .await?
        .into_inner();
    assert_eq!(response.revision, revision);
    assert_eq!(serde_json::from_slice::<Value>(&response.output_json)?, 42);
    Ok(())
}

fn package(language: &str) -> Result<Value> {
    let source = match language {
        "rhai" => "fn invoke(input, context) { input * 2 }",
        "typescript" => "export function invoke(input: number): number { return input * 2; }",
        _ => anyhow::bail!("测试语言无效"),
    };
    Ok(json!({"schema_version":1,"enhancements":[{
        "path":PATH,"language":language,"source":source,
        "input_schema":{"type":"integer"},"output_schema":{"type":"integer"},
        "examples":[{"input":21,"output":42}]
    }]}))
}

async fn head(client: &reqwest::Client, base: &str, cookie: &str) -> Result<Value> {
    Ok(client
        .get(format!("{base}/api/runtime/enhancements"))
        .header("Cookie", cookie)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn publish(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    package: Value,
) -> Result<String> {
    let generation = head(client, base, cookie).await?["generation"].clone();
    let response = client
        .post(format!("{base}/api/runtime/enhancements/revisions"))
        .header("Cookie", cookie)
        .json(&package)
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let revision = response["revision"]
        .as_str()
        .context("缺少候选摘要")?
        .to_owned();
    client
        .put(format!("{base}/api/runtime/enhancements/active"))
        .header("Cookie", cookie)
        .json(&json!({"revision":revision,"expected_generation":generation}))
        .send()
        .await?
        .error_for_status()?;
    Ok(revision)
}
