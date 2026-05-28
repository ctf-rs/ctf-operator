use std::sync::Arc;

use ctf_cli::k8s::{Challenge, Ctf, Flag, Instance};
use futures::StreamExt;
use kube::{
    Api, ResourceExt,
    runtime::{reflector::Store, watcher},
};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::{config::AppConfig, db, k8s::watcher::Watcher, state::AppState};

/// Watchers that populate Stores for app state.
#[derive(Debug)]
pub struct AppWatchers {
    pub instances: Store<Instance>,
    pub flags: Store<Flag>,
    pub challenges: Store<Challenge>,
    pub ctfs: Store<Ctf>,
    instance_watcher: Watcher<Instance>,
    flag_watcher: Watcher<Flag>,
    challenge_watcher: Watcher<Challenge>,
    ctf_watcher: Watcher<Ctf>,
}

impl AppWatchers {
    /// Creates watchers for the configured namespace.
    pub fn new(client: &kube::Client, namespace: &str, ctf_label: &str) -> Self {
        let instance_api: Api<Instance> = Api::namespaced(client.clone(), namespace);
        let instance_watcher = Watcher::new(instance_api, watcher::Config::default());

        // Flags are created in per-instance namespaces, watch cluster-wide with label filter
        let flag_api: Api<Flag> = Api::all(client.clone());
        let flag_config =
            watcher::Config::default().labels(&format!("operators.ctf.rs/ctf={}", ctf_label));
        let flag_watcher = Watcher::new(flag_api, flag_config);

        // Challenge and Ctf - namespaced to the CTF namespace
        let challenge_api: Api<Challenge> = Api::namespaced(client.clone(), namespace);
        let challenge_watcher = Watcher::new(challenge_api, watcher::Config::default());

        let ctf_api: Api<Ctf> = Api::namespaced(client.clone(), namespace);
        let ctf_watcher = Watcher::new(ctf_api, watcher::Config::default());

        Self {
            instances: instance_watcher.store(),
            flags: flag_watcher.store(),
            challenges: challenge_watcher.store(),
            ctfs: ctf_watcher.store(),
            instance_watcher,
            flag_watcher,
            challenge_watcher,
            ctf_watcher,
        }
    }
}

/// Spawns all watchers for the app.
pub fn spawn_sync_workers(
    state: Arc<AppState>,
    config: Arc<AppConfig>,
    watchers: AppWatchers,
) -> Vec<JoinHandle<()>> {
    let ns = config.ctf_namespace.clone();
    let ctf_name = config.ctf_name.clone();

    vec![
        // Instance watcher - just populate Store
        spawn_store_watcher(watchers.instance_watcher, "instance"),
        // Flag watcher with audit logging
        spawn_flag_watcher(Arc::clone(&state), watchers.flag_watcher, ns, ctf_name),
        // Challenge and Ctf watchers - just populate Store
        spawn_store_watcher(watchers.challenge_watcher, "challenge"),
        spawn_store_watcher(watchers.ctf_watcher, "ctf"),
    ]
}

/// Simple watcher that just populates Store (no side effects).
fn spawn_store_watcher<K>(watcher: Watcher<K>, name: &'static str) -> JoinHandle<()>
where
    K: 'static
        + kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + serde::de::DeserializeOwned
        + Send
        + Sync,
{
    tokio::spawn(async move {
        let stream = watcher.to_stream();
        tokio::pin!(stream);

        while let Some(event) = stream.next().await {
            match event {
                Ok(_) => {} // Store is populated automatically by reflect_shared
                Err(err) => {
                    error!(watcher = %name, error = %err, "watcher error, stream continuing");
                }
            }
        }
    })
}

fn spawn_flag_watcher(
    state: Arc<AppState>,
    watcher: Watcher<Flag>,
    source_namespace: String,
    ctf_name: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let stream = watcher.to_stream();
        tokio::pin!(stream);

        while let Some(event) = stream.next().await {
            match event {
                Ok(watcher::Event::Apply(flag)) | Ok(watcher::Event::InitApply(flag)) => {
                    // Log flag generation for audit trail
                    let flag_k8s_name = flag.name_any();
                    let labels = flag.metadata.labels.as_ref();

                    // Only log if the flag has been generated
                    if let Some(generated_flag) = flag.status.as_ref().and_then(|s| s.flag.as_ref())
                    {
                        let Some(instance_name) =
                            labels.and_then(|l| l.get("operators.ctf.rs/instance"))
                        else {
                            warn!(flag = %flag_k8s_name, "flag missing instance label");
                            continue;
                        };
                        let Some(challenge_name) =
                            labels.and_then(|l| l.get("operators.ctf.rs/challenge"))
                        else {
                            warn!(flag = %flag_k8s_name, "flag missing challenge label");
                            continue;
                        };
                        let flag_ns = labels
                            .and_then(|l| l.get("operators.ctf.rs/namespace"))
                            .map(|s| s.as_str())
                            .unwrap_or(&source_namespace);

                        // Extract flag part from the name (after instance name prefix)
                        let flag_part = flag_k8s_name
                            .strip_prefix(&format!("{}-", instance_name))
                            .unwrap_or(&flag_k8s_name);

                        // Look up the instance to get user_id and team_id
                        let instance_key =
                            kube::runtime::reflector::ObjectRef::<Instance>::new(instance_name)
                                .within(flag_ns);
                        let instance_ref = state.instances.get(&instance_key);
                        let user_id = instance_ref
                            .as_ref()
                            .and_then(|inst| inst.spec.user.as_ref().map(|u| u.id));
                        let team_id = instance_ref
                            .as_ref()
                            .and_then(|inst| inst.spec.team.as_ref().map(|t| t.id.clone()));

                        if let Err(err) = db::log_flag_generation(
                            &state.db,
                            &db::FlagGenerationParams {
                                instance_name,
                                user_id,
                                team_id: team_id.as_deref(),
                                challenge_name,
                                ctf_name: &ctf_name,
                                flag_name: flag_part,
                                generated_flag,
                                ignore_case: flag.spec.options.ignore_case,
                                ignore_random: flag.spec.options.ignore_random,
                            },
                        )
                        .await
                        {
                            warn!(
                                flag = %flag_k8s_name,
                                error = %err,
                                "failed to log flag generation"
                            );
                        } else {
                            info!(
                                flag = %flag_k8s_name,
                                user_id = ?user_id,
                                team_id = ?team_id,
                                challenge = %challenge_name,
                                "logged flag generation for audit"
                            );
                        }
                    }
                }
                Ok(_) => {} // Store is populated automatically
                Err(err) => {
                    error!(error = %err, "flag watcher error, stream continuing");
                }
            }
        }
    })
}
