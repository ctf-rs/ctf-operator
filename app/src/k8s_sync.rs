use std::{collections::HashMap, sync::Arc, time::Duration};

use ctf_cli::k8s::{Challenge, ChallengeFileRef, Ctf};
use kube::{Api, ResourceExt, runtime::watcher};
use tokio::{sync::RwLock, task::JoinHandle};
use tokio_stream::StreamExt;
use tracing::{error, info};

use crate::{
    AppState,
    config::AppConfig,
    db::{self, ChallengeView},
};

#[derive(Default)]
struct NamespaceCache {
    challenges: HashMap<String, Challenge>,
    ctfs: HashMap<String, Ctf>,
}

pub fn spawn_sync_workers(state: Arc<AppState>, config: Arc<AppConfig>) -> Vec<JoinHandle<()>> {
    config
        .watch_namespaces
        .iter()
        .map(|namespace| {
            let ns = namespace.clone();
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                run_namespace_sync(state, ns).await;
            })
        })
        .collect()
}

async fn run_namespace_sync(state: Arc<AppState>, namespace: String) {
    let cache = Arc::new(RwLock::new(NamespaceCache::default()));

    let ctf_task = {
        let cache = Arc::clone(&cache);
        let state = Arc::clone(&state);
        let namespace = namespace.clone();
        tokio::spawn(async move {
            let api: Api<Ctf> = Api::namespaced(state.kube.clone(), &namespace);
            loop {
                let stream = watcher::watcher(api.clone(), watcher::Config::default());
                tokio::pin!(stream);
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(watcher::Event::Apply(ctf)) | Ok(watcher::Event::InitApply(ctf)) => {
                            cache.write().await.ctfs.insert(ctf.name_any(), ctf.clone());
                            if let Err(err) = flush_namespace(&state, &cache, &namespace).await {
                                error!(namespace = %namespace, error = %err, "failed to flush ctf sync");
                            }
                        }
                        Ok(watcher::Event::Delete(ctf)) => {
                            cache.write().await.ctfs.remove(&ctf.name_any());
                            if let Err(err) = flush_namespace(&state, &cache, &namespace).await {
                                error!(namespace = %namespace, error = %err, "failed to flush ctf delete");
                            }
                        }
                        Ok(watcher::Event::InitDone) => {}
                        Ok(watcher::Event::Init) => {}
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
                                .insert(challenge.name_any(), challenge.clone());
                            if let Err(err) = flush_namespace(&state, &cache, &namespace).await {
                                error!(namespace = %namespace, error = %err, "failed to flush challenge sync");
                            }
                        }
                        Ok(watcher::Event::Delete(challenge)) => {
                            cache.write().await.challenges.remove(&challenge.name_any());
                            if let Err(err) = flush_namespace(&state, &cache, &namespace).await {
                                error!(namespace = %namespace, error = %err, "failed to flush challenge delete");
                            }
                        }
                        Ok(watcher::Event::InitDone) => {}
                        Ok(watcher::Event::Init) => {}
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

    let mut rows = Vec::new();
    for ctf in cache.ctfs.values() {
        for challenge_name in &ctf.spec.challenges {
            let Some(challenge) = cache.challenges.get(challenge_name) else {
                continue;
            };

            let files = challenge
                .spec
                .files
                .iter()
                .map(|file| match file {
                    ChallengeFileRef::S3 { bucket, key } => {
                        format!("https://{bucket}.s3.amazonaws.com/{key}")
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
    }

    db::replace_challenges_for_namespace(&state.db, namespace, &rows).await
}
