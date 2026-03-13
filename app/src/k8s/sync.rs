use std::{collections::HashMap, sync::Arc, time::Duration};

use ctf_cli::k8s::{Challenge, ChallengeFileRef, Ctf};
use kube::{Api, ResourceExt, runtime::watcher};
use tokio::{sync::RwLock, task::JoinHandle};
use tokio_stream::StreamExt;
use tracing::{error, info};

use crate::{
    config::AppConfig,
    db::{self, ChallengeView},
    state::AppState,
};

#[derive(Default)]
struct NamespaceCache {
    challenges: HashMap<String, Challenge>,
    ctf: Option<Ctf>,
}

/// Spawns watchers for the single configured CTF namespace.
pub fn spawn_sync_worker(state: Arc<AppState>, config: Arc<AppConfig>) -> Vec<JoinHandle<()>> {
    let ns = config.ctf_namespace.clone();
    let ctf_name = config.ctf_name.clone();

    vec![tokio::spawn(async move {
        run_namespace_sync(state, ns, ctf_name).await;
    })]
}

async fn run_namespace_sync(state: Arc<AppState>, namespace: String, ctf_name: String) {
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
                            if let Err(err) =
                                flush_namespace(&state, &cache, &namespace).await
                            {
                                error!(namespace = %namespace, error = %err, "failed to flush challenge sync");
                            }
                        }
                        Ok(watcher::Event::Delete(challenge)) => {
                            cache.write().await.challenges.remove(&challenge.name_any());
                            if let Err(err) =
                                flush_namespace(&state, &cache, &namespace).await
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

    info!(namespace = %namespace, "started ctf/challenge watchers");
    let _ = tokio::join!(ctf_task, challenge_task);
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
            .map(|flag| flag.name.clone())
            .collect::<Vec<_>>();

        rows.push(ChallengeView {
            namespace: namespace.to_string(),
            name: challenge.name_any(),
            ctf_name: ctf.name_any(),
            ctf_public: ctf.spec.public,
            full_name: challenge.spec.full_name.clone(),
            description: challenge.spec.description.clone(),
            files,
            flags,
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

    format!("{endpoint}/{bucket}/{key}")
}
