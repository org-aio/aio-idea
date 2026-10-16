use anyhow::Result;
use az_plugin_host::{
    authorization::AuthorizationProvider,
    identity::{IdentityProvider, SessionContext},
    metering::{MeterOutcome, MeteringProvider},
};
use std::sync::Arc;

pub struct ProductIdentity {
    identity: Arc<aio_plugin_identity_server::IdentityService>,
    pool: sqlx::PgPool,
}

impl ProductIdentity {
    pub fn new(
        identity: Arc<aio_plugin_identity_server::IdentityService>,
        database: &str,
    ) -> Result<Self> {
        Ok(Self {
            identity,
            pool: sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect_lazy(database)?,
        })
    }
}

#[async_trait::async_trait]
impl IdentityProvider for ProductIdentity {
    async fn member_active(&self, tenant: &str, user: &str) -> Result<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenant_memberships m JOIN identity_users u ON u.id=m.user_id WHERE m.tenant_id=$1 AND m.user_id=$2)").bind(tenant).bind(user).fetch_one(&self.pool).await?)
    }
    async fn session_active(&self, session: &str, tenant: &str, user: &str) -> Result<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM auth_sessions WHERE id=$1 AND tenant_id=$2 AND user_id=$3 AND expires_at>now())").bind(session).bind(tenant).bind(user).fetch_one(&self.pool).await?)
    }
    async fn authenticate(
        &self,
        headers: &axum::http::HeaderMap,
    ) -> Result<Option<SessionContext>> {
        Ok(self
            .identity
            .authenticate(headers)
            .await?
            .map(|session| SessionContext {
                session_id: session.session_id,
                user_id: session.user_id,
                account: session.account,
                display_name: session.display_name,
                tenant_id: session.tenant_id,
                tenant_label: session.tenant_label,
                permissions: session.permissions,
            }))
    }
}

#[async_trait::async_trait]
impl AuthorizationProvider for ProductIdentity {
    async fn can_publish(&self, session: &SessionContext) -> Result<bool> {
        Ok(publish_account_allowed(
            std::env::var("AIO_PLUGIN_PUBLISH_ACCOUNTS").ok().as_deref(),
            &session.account,
        ))
    }
    async fn session_has_permissions(
        &self,
        session: &str,
        tenant: &str,
        user: &str,
        permissions: &[String],
    ) -> Result<bool> {
        // 演进提交读取当前会话与角色关系，不沿用模型生成之前的权限快照。
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM auth_sessions s JOIN tenant_memberships m ON m.tenant_id=s.tenant_id AND m.user_id=s.user_id JOIN tenant_member_roles r ON r.tenant_id=m.tenant_id AND r.user_id=m.user_id JOIN role_permissions p ON p.tenant_id=r.tenant_id AND p.role_id=r.role_id WHERE s.id=$1 AND s.tenant_id=$2 AND s.user_id=$3 AND s.expires_at>now() AND p.permission=ANY($4) GROUP BY s.id HAVING count(DISTINCT p.permission)=cardinality($4::text[]))")
            .bind(session).bind(tenant).bind(user).bind(permissions).fetch_one(&self.pool).await?)
    }
}

#[async_trait::async_trait]
impl MeteringProvider for ProductIdentity {
    async fn meter(
        &self,
        tenant: &str,
        user: &str,
        source: &str,
        resource: &str,
        quantity: i64,
        idempotency_key: &str,
    ) -> Result<MeterOutcome> {
        // 宿主与身份插件同库同进程，直接调用计费 Service，避免再走 HTTP 票据。
        let outcome = self
            .identity
            .meter_resource(tenant, user, source, resource, quantity, idempotency_key)
            .await?;
        Ok(MeterOutcome {
            amount_micros: outcome.amount_micros,
            grant_consumed: outcome.grant_consumed,
            balance_charged_micros: outcome.balance_charged_micros,
            balance_after_micros: outcome.balance_after_micros,
            duplicate: outcome.duplicate,
        })
    }
}

fn publish_account_allowed(configured: Option<&str>, account: &str) -> bool {
    configured.is_some_and(|configured| {
        configured
            .split(',')
            .map(str::trim)
            .any(|candidate| !candidate.is_empty() && candidate == account)
    })
}

/// 当前代的产品身份服务贡献身份、授权与计费接口；连接池由本插件关闭。
pub struct ProductIdentityPlugin;

impl az_plugin_core::Plugin<az_plugin_host::runtime::server::plugins::RuntimeAssembly>
    for ProductIdentityPlugin
{
    fn build<'a>(
        &'a self,
        assembly: &'a mut az_plugin_host::runtime::server::plugins::RuntimeAssembly,
    ) -> az_plugin_core::PluginFuture<'a> {
        Box::pin(async move {
            use az_plugin_host::runtime::server::plugins::Resource;
            assembly.require_service::<aio_plugin_identity_server::IdentityService>();
            assembly.declare_resource::<ProductIdentity>()?;
            assembly
                .services
                .bind::<dyn IdentityProvider, ProductIdentity>()
                .bind::<dyn AuthorizationProvider, ProductIdentity>()
                .bind::<dyn MeteringProvider, ProductIdentity>();
            assembly.require_service::<dyn IdentityProvider>();
            assembly.require_service::<dyn AuthorizationProvider>();
            assembly.require_service::<dyn MeteringProvider>();
            assembly.on_initialize::<Self>(|state, _| {
                Box::pin(async move {
                    let provider = Arc::new(ProductIdentity::new(
                        state.service::<aio_plugin_identity_server::IdentityService>()?,
                        &state.config.database_url,
                    )?);
                    // 先交给资源槽，连接验证失败时也由停止钩子关闭本次创建的池。
                    state
                        .service::<Resource<ProductIdentity>>()?
                        .provide(provider.clone())?;
                    sqlx::query("SELECT 1").execute(&provider.pool).await?;
                    Ok(())
                })
            });
            assembly.on_stop::<Self>(|state, _| {
                Box::pin(async move {
                    if let Some(provider) =
                        state.service::<Resource<ProductIdentity>>()?.initialized()
                    {
                        provider.pool.close().await;
                    }
                    Ok(())
                })
            });
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod lifecycle_tests;
