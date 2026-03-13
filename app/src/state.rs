use std::{collections::HashMap, sync::Arc};

use kube::Client;
use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::{config::AppConfig, oauth};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: PgPool,
    pub kube: Client,
    pub oidc_client: oauth::OidcClient,
    pub pending_oauth: Arc<RwLock<HashMap<String, oauth::PendingAuth>>>,
}
