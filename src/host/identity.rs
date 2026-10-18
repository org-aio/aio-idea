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

    /// 在指定租户下创建账号；身份插件负责账号唯一性与密码策略。
    async fn create_user(
        &self,
        tenant: &str,
        account: &str,
        display_name: &str,
        password: &str,
    ) -> Result<String> {
        self.identity
            .create_user(tenant, account, display_name, password)
            .await
    }

    /// 重置账号密码；目标归属由宿主身份通道在调用前复核。
    async fn reset_password(&self, user_id: &str, new_password: &str) -> Result<()> {
        self.identity.reset_password(user_id, new_password).await
    }

    /// 切换会话租户后重新读取会话，返回调用方需要的新上下文。
    async fn switch_tenant(
        &self,
        session: &SessionContext,
        tenant_id: &str,
    ) -> Result<Option<SessionContext>> {
        let switched = self
            .identity
            .switch_tenant(
                &aio_plugin_identity_server::SessionContext {
                    session_id: session.session_id.clone(),
                    user_id: session.user_id.clone(),
                    account: session.account.clone(),
                    display_name: session.display_name.clone(),
                    tenant_id: session.tenant_id.clone(),
                    tenant_label: session.tenant_label.clone(),
                    permissions: session.permissions.clone(),
                },
                tenant_id,
            )
            .await?;
        if !switched {
            return Ok(None);
        }
        // 会话表已更新，按主键重读以拿到新租户标签与权限，避免沿用旧快照。
        let row = sqlx::query_as::<_, (String, String, String, String, String, String)>(
            "SELECT s.id, s.user_id, u.account, u.display_name, t.id, t.label FROM auth_sessions s JOIN identity_users u ON u.id = s.user_id JOIN tenants t ON t.id = s.tenant_id WHERE s.id = $1 AND s.expires_at > now()",
        )
        .bind(&session.session_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((session_id, user_id, account, display_name, tenant, tenant_label)) = row else {
            return Ok(None);
        };
        let permissions = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT permission FROM role_permissions WHERE tenant_id = $1 AND role_id IN (SELECT role_id FROM tenant_member_roles WHERE tenant_id = $1 AND user_id = $2) ORDER BY permission",
        )
        .bind(&tenant)
        .bind(&user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(SessionContext {
            session_id,
            user_id,
            account,
            display_name,
            tenant_id: tenant,
            tenant_label,
            permissions,
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
