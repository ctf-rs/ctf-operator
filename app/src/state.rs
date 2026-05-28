use std::{collections::HashMap, sync::Arc};

use ctf_cli::k8s::{Challenge, Ctf, Flag, Instance};
use kube::{Client, runtime::reflector::Store};
use sea_orm::DatabaseConnection;
use tokio::sync::RwLock;

use crate::{config::AppConfig, oauth, scoreboard::Scoreboard};

#[derive(Clone)]
#[expect(
    missing_debug_implementations,
    reason = "kube Store doesn't impl Debug"
)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: DatabaseConnection,
    pub kube: Client,
    pub oidc_client: oauth::OidcClient,
    pub pending_oauth: Arc<RwLock<HashMap<String, oauth::PendingAuth>>>,
    /// Store of Instance resources from k8s watcher
    pub instances: Store<Instance>,
    /// Store of Flag resources from k8s watcher
    pub flags: Store<Flag>,
    /// Store of Challenge resources from k8s watcher
    pub challenges: Store<Challenge>,
    /// Store of Ctf resources from k8s watcher
    pub ctfs: Store<Ctf>,
    /// Scoreboard cache (points, leaderboard) — visible to players.
    ///
    /// When the scoreboard is frozen this cache stops updating and serves
    /// the last pre-freeze snapshot.
    pub scoreboard: Arc<RwLock<Scoreboard>>,
    /// Live scoreboard cache — always fresh, never freezes.
    ///
    /// Only accessible to admins via `/scoreboard/live`.
    pub live_scoreboard: Arc<RwLock<Scoreboard>>,
}
