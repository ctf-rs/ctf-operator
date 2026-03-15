use std::{collections::HashMap, sync::Arc, time::Duration};

use ctf_cli::k8s::{Challenge, ChallengeFileRef, Ctf, Flag, Instance};
use futures::StreamExt;
use kube::{
    Api, ResourceExt,
    runtime::{reflector::Store, watcher},
};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::{error, info, warn};

use crate::{
    config::AppConfig,
    db::{self, ChallengeView, FlagView, PortView},
    k8s::watcher::Watcher,
    state::AppState,
};

#[derive(Default)]
struct NamespaceCache {
    challenges: HashMap<String, Challenge>,
    ctf: Option<Ctf>,
}

/// Watchers that populate Stores for app state.
pub struct AppWatchers {
    pub instances: Store<Instance>,
    pub flags: Store<Flag>,
    instance_watcher: Watcher<Instance>,
    flag_watcher: Watcher<Flag>,
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

        Self {
            instances: instance_watcher.store(),
            flags: flag_watcher.store(),
            instance_watcher,
            flag_watcher,
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
        // Ctf/Challenge sync to DB
        spawn_ctf_challenge_sync(Arc::clone(&state), ns.clone(), ctf_name.clone()),
        // Instance watcher with DB cleanup on deletion
        spawn_instance_watcher(
            Arc::clone(&state),
            watchers.instance_watcher,
            ctf_name.clone(),
        ),
        // Flag watcher with audit logging
        spawn_flag_watcher(Arc::clone(&state), watchers.flag_watcher, ns, ctf_name),
    ]
}

fn spawn_ctf_challenge_sync(
    state: Arc<AppState>,
    namespace: String,
    ctf_name: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let cache = Arc::new(RwLock::new(NamespaceCache::default()));

        let ctf_task = {
            let cache = Arc::clone(&cache);
            let state = Arc::clone(&state);
            let namespace = namespace.clone();
            let ctf_name = ctf_name.clone();
            tokio::spawn(async move {
                let api: Api<Ctf> = Api::namespaced(state.kube.clone(), &namespace);
                loop {
                    let stream = watcher::watcher(api.clone(), watcher::Config::default());
                    tokio::pin!(stream);
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(watcher::Event::Apply(ctf)) | Ok(watcher::Event::InitApply(ctf)) => {
                                if ctf.name_any() == ctf_name {
                                    cache.write().await.ctf = Some(ctf);
                                    if let Err(err) =
                                        flush_namespace(&state, &cache, &namespace).await
                                    {
                                        error!(namespace = %namespace, error = %err, "failed to flush ctf sync");
                                    }
                                }
                            }
                            Ok(watcher::Event::Delete(ctf)) => {
                                if ctf.name_any() == ctf_name {
                                    cache.write().await.ctf = None;
                                    if let Err(err) =
                                        flush_namespace(&state, &cache, &namespace).await
                                    {
                                        error!(namespace = %namespace, error = %err, "failed to flush ctf delete");
                                    }
                                }
                            }
                            Ok(watcher::Event::InitDone | watcher::Event::Init) => {}
                            Err(err) => {
                                error!(namespace = %namespace, error = %err, "ctf watcher error");
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                break;
                            }
                        }
                    }
                }
            })
        };

        let challenge_task = {
            let cache = Arc::clone(&cache);
            let state = Arc::clone(&state);
            let namespace = namespace.clone();
            tokio::spawn(async move {
                let api: Api<Challenge> = Api::namespaced(state.kube.clone(), &namespace);
                loop {
                    let stream = watcher::watcher(api.clone(), watcher::Config::default());
                    tokio::pin!(stream);
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(watcher::Event::Apply(challenge))
                            | Ok(watcher::Event::InitApply(challenge)) => {
                                cache
                                    .write()
                                    .await
                                    .challenges
                                    .insert(challenge.name_any(), challenge);
                                if let Err(err) = flush_namespace(&state, &cache, &namespace).await
                                {
                                    error!(namespace = %namespace, error = %err, "failed to flush challenge sync");
                                }
                            }
                            Ok(watcher::Event::Delete(challenge)) => {
                                cache.write().await.challenges.remove(&challenge.name_any());
                                if let Err(err) = flush_namespace(&state, &cache, &namespace).await
                                {
                                    error!(namespace = %namespace, error = %err, "failed to flush challenge delete");
                                }
                            }
                            Ok(watcher::Event::InitDone | watcher::Event::Init) => {}
                            Err(err) => {
                                error!(namespace = %namespace, error = %err, "challenge watcher error");
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                break;
                            }
                        }
                    }
                }
            })
        };

        info!(namespace = %namespace, "started ctf/challenge DB sync watchers");
        let _ = tokio::join!(ctf_task, challenge_task);
    })
}

fn spawn_instance_watcher(
    state: Arc<AppState>,
    watcher: Watcher<Instance>,
    ctf_name: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let stream = watcher.to_stream();
        tokio::pin!(stream);

        while let Some(event) = stream.next().await {
            match event {
                Ok(watcher::Event::Delete(instance)) => {
                    // Clean up DB record for this instance
                    if let Some(user_id_str) = instance
                        .metadata
                        .annotations
                        .as_ref()
                        .and_then(|a| a.get("app.ctf.rs/user-id"))
                    {
                        if let Ok(user_id) = user_id_str.parse::<i64>() {
                            let challenge_name = &instance.spec.challenge;
                            if let Err(err) = db::clear_instance_for_user(
                                &state.db,
                                user_id,
                                challenge_name,
                                &ctf_name,
                            )
                            .await
                            {
                                warn!(
                                    instance = %instance.name_any(),
                                    user_id = %user_id,
                                    error = %err,
                                    "failed to clean DB on instance delete"
                                );
                            } else {
                                info!(
                                    instance = %instance.name_any(),
                                    user_id = %user_id,
                                    "cleaned DB record on instance deletion"
                                );
                            }
                        }
                    }
                }
                Ok(_) => {} // Store is populated automatically by reflect_shared
                Err(err) => {
                    error!(error = %err, "instance watcher error, stream continuing");
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

                        // Look up the instance to get user_id
                        let instance_key =
                            kube::runtime::reflector::ObjectRef::<Instance>::new(instance_name)
                                .within(flag_ns);
                        let user_id = state.instances.get(&instance_key).and_then(|inst| {
                            inst.metadata
                                .annotations
                                .as_ref()
                                .and_then(|a| a.get("app.ctf.rs/user-id"))
                                .and_then(|s| s.parse::<i64>().ok())
                        });

                        if let Err(err) = db::log_flag_generation(
                            &state.db,
                            instance_name,
                            user_id,
                            challenge_name,
                            &ctf_name,
                            flag_part,
                            generated_flag,
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

async fn flush_namespace(
    state: &Arc<AppState>,
    cache: &Arc<RwLock<NamespaceCache>>,
    namespace: &str,
) -> anyhow::Result<()> {
    let cache = cache.read().await;

    let Some(ctf) = cache.ctf.as_ref() else {
        return db::replace_challenges_for_namespace(&state.db, namespace, &[]).await;
    };

    let mut rows = Vec::new();
    for challenge_name in &ctf.spec.challenges {
        let Some(challenge) = cache.challenges.get(challenge_name) else {
            continue;
        };

        let files = challenge
            .spec
            .files
            .iter()
            .map(|file| match file {
                ChallengeFileRef::S3 {
                    bucket,
                    key,
                    endpoint,
                    path_style,
                } => {
                    let effective_endpoint = endpoint
                        .as_deref()
                        .or(state.config.challenge_files_s3_endpoint.as_deref());
                    let effective_path_style = if endpoint.is_some() {
                        *path_style
                    } else {
                        state.config.challenge_files_s3_path_style
                    };
                    render_s3_url(bucket, key, effective_endpoint, effective_path_style)
                }
            })
            .collect::<Vec<_>>();

        let flags = challenge
            .spec
            .flags
            .iter()
            .map(|flag| FlagView {
                name: flag.name.clone(),
                label: flag.label.clone(),
                solved: false, // Will be populated from DB later
                base_points: flag.scoring.base_points,
                min_points: flag.scoring.min_points,
                decay: flag.scoring.decay,
            })
            .collect::<Vec<_>>();

        // Collect exposed ports from all containers
        let exposed_ports: Vec<PortView> = challenge
            .spec
            .containers
            .iter()
            .flat_map(|container| {
                container
                    .ports
                    .iter()
                    .filter(|p| p.expose)
                    .map(|p| PortView {
                        name: p.name.clone(),
                        port: p.container_port,
                        port_type: format!("{:?}", p.port_type).to_lowercase(),
                        allocated_port: None, // Will be populated from PortClaim when viewing instance
                    })
            })
            .collect();

        let categories: Vec<_> = challenge.spec.categories.iter().copied().collect();

        let difficulty = challenge.spec.difficulty;

        rows.push(ChallengeView {
            namespace: namespace.to_string(),
            name: challenge.name_any(),
            ctf_name: ctf.name_any(),
            ctf_public: ctf.spec.public,
            full_name: challenge.spec.full_name.clone(),
            description: challenge.spec.description.clone(),
            categories,
            difficulty,
            files,
            flags,
            exposed_ports,
            flag_hint: String::new(), // TODO: add to CRD
        });
    }

    db::replace_challenges_for_namespace(&state.db, namespace, &rows).await
}

fn render_s3_url(bucket: &str, key: &str, endpoint: Option<&str>, path_style: bool) -> String {
    let key = key.trim_start_matches('/');
    let Some(endpoint) = endpoint.map(str::trim).filter(|value| !value.is_empty()) else {
        return format!("https://{bucket}.s3.amazonaws.com/{key}");
    };

    let endpoint = endpoint.trim_end_matches('/');
    if path_style {
        return format!("{endpoint}/{bucket}/{key}");
    }

    if let Some(rest) = endpoint.strip_prefix("https://") {
        return format!("https://{bucket}.{rest}/{key}");
    }

    if let Some(rest) = endpoint.strip_prefix("http://") {
        return format!("http://{bucket}.{rest}/{key}");
    }

    // Fallback: assume https and virtual-hosted style
    format!("https://{bucket}.{endpoint}/{key}")
}
