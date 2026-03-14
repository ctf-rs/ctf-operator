use std::{collections::HashMap, sync::Arc};

use ctf_cli::k8s::Instance;
use kube::Client;
use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::{config::AppConfig, oauth};

/// Key for instance cache: (namespace, instance_name)
pub type InstanceKey = (String, String);

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: PgPool,
    pub kube: Client,
    pub oidc_client: oauth::OidcClient,
    pub pending_oauth: Arc<RwLock<HashMap<String, oauth::PendingAuth>>>,
    /// Cache of Instance resources from k8s watcher
    pub instance_cache: Arc<RwLock<HashMap<InstanceKey, Instance>>>,
}
