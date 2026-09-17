use anyhow::Result;
use az_plugin_host::identity::{IdentityProvider, SessionContext};
use std::sync::Arc;

pub struct ProductIdentity {
    identity: Arc<aio_plugin_identity_server::IdentityService>,
    pool: sqlx::PgPool,
}

impl ProductIdentity {
    pub async fn new(
        identity: Arc<aio_plugin_identity_server::IdentityService>,
        database: &str,
    ) -> Result<Self> {
        Ok(Self {
            identity,
            pool: sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect(database)
                .await?,
        })
    }
}

#[async_trait::async_trait]
impl IdentityProvider for ProductIdentity {
    async fn can_publish(&self, session: &SessionContext) -> Result<bool> {
        Ok(publish_account_allowed(
            std::env::var("AIO_PLUGIN_PUBLISH_ACCOUNTS").ok().as_deref(),
            &session.account,
        ))
    }
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

fn publish_account_allowed(configured: Option<&str>, account: &str) -> bool {
    configured.is_some_and(|configured| {
        configured
            .split(',')
            .map(str::trim)
            .any(|candidate| !candidate.is_empty() && candidate == account)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publishing_requires_an_explicit_product_account() {
        assert!(!publish_account_allowed(None, "zjarlin"));
        assert!(publish_account_allowed(Some("alice, zjarlin"), "zjarlin"));
        assert!(!publish_account_allowed(
            Some("alice,zjarlin-admin"),
            "zjarlin"
        ));
    }
}
