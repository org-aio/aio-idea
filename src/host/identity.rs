use anyhow::Result;
use az_plugin_host::{
    authorization::AuthorizationProvider,
    identity::{
        AccessControlDepartment, AccessControlOrganization, AccessControlPost,
        AccessControlProvider, AccessControlRole, AccessControlUser, AccessControlView,
        IdentityProvider, SessionContext,
    },
    metering::{MeterOutcome, MeteringProvider},
};
use std::sync::Arc;

pub struct ProductIdentity {
    identity: Arc<aio_plugin_identity_server::IdentityService>,
    pool: sqlx::PgPool,
    /// 产品复用权限插件同一套授权模型，避免复制 SQL 或维护第二套判据。
    access: aio_plugin_rbac_server::AccessControlService,
}

impl ProductIdentity {
    pub fn new(
        identity: Arc<aio_plugin_identity_server::IdentityService>,
        database: &str,
    ) -> Result<Self> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_lazy(database)?;
        Ok(Self {
            identity,
            access: aio_plugin_rbac_server::AccessControlService::from_pool(pool.clone()),
            pool,
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
    /// 列出该用户加入的租户，并标记会话当前所在租户。
    async fn list_tenants(
        &self,
        session: &SessionContext,
    ) -> Result<Vec<az_plugin_host::identity::TenantView>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT tenants.id, tenants.label FROM tenant_memberships memberships JOIN tenants ON tenants.id = memberships.tenant_id WHERE memberships.user_id = $1 ORDER BY tenants.label",
        )
        .bind(&session.user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, label)| az_plugin_host::identity::TenantView {
                current: id == session.tenant_id,
                id,
                label,
            })
            .collect())
    }

    /// 创建租户：新租户里只放调用者，并授予它管理员角色与基础权限。
    async fn create_tenant(
        &self,
        session: &SessionContext,
        label: &str,
    ) -> Result<az_plugin_host::identity::TenantView> {
        let label = label.trim();
        anyhow::ensure!(
            !label.is_empty() && label.chars().count() <= 80,
            "租户名称需要 1 到 80 个字符"
        );
        let mut transaction = self.pool.begin().await?;
        // 租户 ID 由数据库生成，避免为宿主进程新增 UUID 依赖，也避免多进程竞争。
        let tenant_id: String = sqlx::query_scalar(
            "INSERT INTO tenants (id, label) VALUES (gen_random_uuid()::text, $1) RETURNING id",
        )
        .bind(label)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO tenant_memberships (tenant_id, user_id, display_name) VALUES ($1, $2, $3)",
        )
        .bind(&tenant_id)
        .bind(&session.user_id)
        .bind(&session.display_name)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO tenant_member_roles (tenant_id, user_id, role_id) VALUES ($1, $2, 'tenant-admin')")
            .bind(&tenant_id)
            .bind(&session.user_id)
            .execute(&mut *transaction)
            .await?;
        // 新租户的最小可用权限集合保持与既有产品语义一致。
        for permission in [
            "plugin:manage",
            "tenant:manage",
            "rbac:manage",
            "dictionary:manage",
            "file:manage",
        ] {
            sqlx::query("INSERT INTO role_permissions (tenant_id, role_id, permission) VALUES ($1, 'tenant-admin', $2)")
                .bind(&tenant_id)
                .bind(permission)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query("INSERT INTO role_permissions (tenant_id, role_id, permission) VALUES ($1, 'member', 'workspace:view')")
            .bind(&tenant_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(az_plugin_host::identity::TenantView {
            id: tenant_id,
            label: label.to_owned(),
            current: false,
        })
    }

    /// 只允许重命名会话当前所在租户，避免越权改动其他租户。
    async fn rename_current_tenant(
        &self,
        session: &SessionContext,
        label: &str,
    ) -> Result<az_plugin_host::identity::TenantView> {
        let label = label.trim();
        anyhow::ensure!(
            !label.is_empty() && label.chars().count() <= 80,
            "租户名称需要 1 到 80 个字符"
        );
        let updated = sqlx::query("UPDATE tenants SET label = $2 WHERE id = $1")
            .bind(&session.tenant_id)
            .bind(label)
            .execute(&self.pool)
            .await?;
        anyhow::ensure!(updated.rows_affected() == 1, "租户不存在");
        Ok(az_plugin_host::identity::TenantView {
            id: session.tenant_id.clone(),
            label: label.to_owned(),
            current: true,
        })
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
                .bind::<dyn MeteringProvider, ProductIdentity>()
                .bind::<dyn AccessControlProvider, ProductIdentity>();
            assembly.require_service::<dyn IdentityProvider>();
            assembly.require_service::<dyn AuthorizationProvider>();
            assembly.require_service::<dyn MeteringProvider>();
            assembly.require_service::<dyn AccessControlProvider>();
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
                    provider.access.initialize().await?;
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

/// 受控权限提供者：直接复用权限插件的授权模型与事务判据。
#[async_trait::async_trait]
impl AccessControlProvider for ProductIdentity {
    async fn access_control_view(&self, session: &SessionContext) -> Result<AccessControlView> {
        let view = self
            .access
            .view(&session.tenant_id, &session.user_id)
            .await?;
        Ok(to_contract_view(view))
    }

    async fn save_role(
        &self,
        session: &SessionContext,
        role_id: &str,
        permissions: &[String],
        create: bool,
    ) -> Result<()> {
        self.access
            .save_role(
                &session.tenant_id,
                &session.user_id,
                role_id,
                permissions,
                create,
            )
            .await
    }

    async fn delete_role(&self, session: &SessionContext, role_id: &str) -> Result<()> {
        self.access
            .delete_role(&session.tenant_id, &session.user_id, role_id)
            .await
    }

    async fn update_member(
        &self,
        session: &SessionContext,
        user_id: &str,
        display_name: &str,
    ) -> Result<()> {
        self.access
            .update_member(&session.tenant_id, &session.user_id, user_id, display_name)
            .await
    }

    async fn set_member_roles(
        &self,
        session: &SessionContext,
        user_id: &str,
        roles: &[String],
    ) -> Result<()> {
        self.access
            .set_member_roles(&session.tenant_id, &session.user_id, user_id, roles, false)
            .await
    }

    async fn remove_member(&self, session: &SessionContext, user_id: &str) -> Result<()> {
        self.access
            .remove_member(&session.tenant_id, &session.user_id, user_id)
            .await
    }

    async fn authorize_member_password_reset(
        &self,
        session: &SessionContext,
        user_id: &str,
    ) -> Result<()> {
        self.access
            .authorize_member_password_reset(&session.tenant_id, &session.user_id, user_id)
            .await
    }

    async fn save_department(
        &self,
        session: &SessionContext,
        department: AccessControlDepartment,
    ) -> Result<()> {
        self.access
            .save_department(
                &session.tenant_id,
                &session.user_id,
                aio_plugin_rbac_model::SaveDepartmentRequest {
                    id: department.id,
                    parent_id: department.parent_id,
                    name: department.name,
                    leader: department.leader,
                    phone: department.phone,
                    email: department.email,
                    sort_order: department.sort_order,
                    status: department.status,
                },
            )
            .await
    }

    async fn delete_department(&self, session: &SessionContext, id: &str) -> Result<()> {
        self.access
            .delete_department(&session.tenant_id, &session.user_id, id)
            .await
    }

    async fn save_post(&self, session: &SessionContext, post: AccessControlPost) -> Result<()> {
        self.access
            .save_post(
                &session.tenant_id,
                &session.user_id,
                aio_plugin_rbac_model::SavePostRequest {
                    id: post.id,
                    code: post.code,
                    name: post.name,
                    sort_order: post.sort_order,
                    status: post.status,
                    remark: post.remark,
                },
            )
            .await
    }

    async fn delete_post(&self, session: &SessionContext, id: &str) -> Result<()> {
        self.access
            .delete_post(&session.tenant_id, &session.user_id, id)
            .await
    }
}

fn to_contract_view(view: aio_plugin_rbac_model::AccessControlView) -> AccessControlView {
    AccessControlView {
        users: view
            .users
            .into_iter()
            .map(|user| AccessControlUser {
                id: user.id,
                account: user.account,
                display_name: user.display_name,
                roles: user.roles,
            })
            .collect(),
        roles: view
            .roles
            .into_iter()
            .map(|role| AccessControlRole {
                id: role.id,
                permissions: role.permissions,
                member_count: role.member_count,
            })
            .collect(),
        departments: view.departments.into_iter().map(to_contract_org).collect(),
        posts: view.posts.into_iter().map(to_contract_org).collect(),
        current_user_id: view.current_user_id,
        grantable_permissions: view.grantable_permissions,
    }
}

fn to_contract_org(item: aio_plugin_rbac_model::OrganizationItem) -> AccessControlOrganization {
    AccessControlOrganization {
        id: item.id,
        code: item.code,
        name: item.name,
        parent_id: item.parent_id,
        leader: item.leader,
        phone: item.phone,
        email: item.email,
        sort_order: item.sort_order,
        status: item.status,
        remark: item.remark,
    }
}
