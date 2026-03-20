use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_cli::k8s::{Challenge, ChallengeStatus, ConditionStatus, Instance, InstanceSpec};
use ctf_cli::models::{ChallengeContainer, ChallengeFlag, ContainerImage};
use kube::{
    api::{ObjectMeta, Patch, PatchParams},
    runtime::controller::Action,
    Api, ResourceExt,
};
use tracing::info;

#[derive(Debug)]
pub struct ChallengeReconciler;

impl Reconciler for ChallengeReconciler {
    const NAME: &'static str = "challenge-controller";
    type K = Challenge;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(resource: Arc<Challenge>, ctx: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = resource.namespace().unwrap_or_default();
        if !ctx.watches_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let mut status = resource.status.clone().unwrap_or_default();
        validate_spec(&resource, &mut status);

        let ready = status.conditions.all_true();

        if resource.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(resource.as_ref(), ctx.client.clone(), status).await?;
        }

        if ready {
            // Auto-create global instance for non-instanced challenges that
            // actually provide containers. If a challenge has no containers
            // we never create an instance (e.g., informational challenges).
            if !resource.spec.instancing && !resource.spec.containers.is_empty() {
                match ensure_global_instance(&resource, &namespace, &ctx).await {
                    Ok(Some(requeue)) => return Ok(Action::requeue(requeue)),
                    Ok(None) => {}
                    Err(err) => {
                        tracing::warn!(error = %err, challenge = %resource.name_any(), "failed to ensure global instance");
                        return Ok(Action::requeue(Duration::from_secs(15)));
                    }
                }
            }
            Ok(Action::await_change())
        } else {
            Ok(Action::requeue(Duration::from_secs(
                Self::ERROR_RECONCILE_REQUEUE_TIME,
            )))
        }
    }
}

/// Ensure a global (shared) `Instance` exists for a non-instanced challenge.
/// The instance has `user=None` and `duration_seconds=None` (infinite).
///
/// Returns `Ok(Some(duration))` when the caller should requeue after that
/// duration (e.g. the cached instance is being deleted), or `Ok(None)` when
/// nothing further is needed.
async fn ensure_global_instance(
    challenge: &Challenge,
    namespace: &str,
    state: &CtfState,
) -> Result<Option<Duration>, CtfError> {
    // Find the CTF that references this challenge
    let ctf = state.ctfs.state().into_iter().find(|c| {
        c.namespace().as_deref() == Some(namespace)
            && c.spec
                .challenges
                .iter()
                .any(|ch| ch == &challenge.name_any())
    });
    let Some(ctf) = ctf else {
        // Challenge not yet referenced by any CTF — skip
        return Ok(None);
    };

    let ctf_name = ctf.name_any();
    let challenge_name = challenge.name_any();

    // Deterministic name: same format as the app uses
    let instance_name = format!(
        "{}-{}-{}",
        state.config.instance_namespace_prefix.trim_end_matches('-'),
        sanitize_k8s_name(&ctf_name),
        sanitize_k8s_name(&challenge_name),
    );
    let instance_name: String = instance_name.chars().take(63).collect();

    // Check if instance already exists in the reflector store
    let cached = state.instances.state().into_iter().find(|inst| {
        inst.name_any() == instance_name && inst.namespace().as_deref() == Some(namespace)
    });
    if let Some(inst) = cached {
        if inst.metadata.deletion_timestamp.is_some() {
            // Instance is being deleted — requeue so we recreate once it's gone
            info!(
                challenge = %challenge_name,
                instance = %instance_name,
                "global instance is being deleted, will requeue"
            );
            return Ok(Some(Duration::from_secs(5)));
        }
        // Instance exists and is not being deleted
        return Ok(None);
    }

    info!(
        challenge = %challenge_name,
        instance = %instance_name,
        "auto-creating global instance for non-instanced challenge"
    );

    let instance = Instance {
        metadata: ObjectMeta {
            name: Some(instance_name.clone()),
            namespace: Some(namespace.to_owned()),
            ..Default::default()
        },
        spec: InstanceSpec {
            ctf: ctf_name,
            challenge: challenge_name,
            user: None,
            duration_seconds: None,
        },
        status: None,
    };

    let api: Api<Instance> = Api::namespaced(state.client.clone(), namespace);
    api.patch(
        instance.metadata.name.as_deref().unwrap_or_default(),
        &PatchParams::apply(ChallengeReconciler::NAME).force(),
        &Patch::Apply(&instance),
    )
    .await?;

    Ok(None)
}

fn sanitize_k8s_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "challenge".to_string()
    } else {
        out
    }
}

fn validate_spec(resource: &Challenge, status: &mut ChallengeStatus) {
    let core = ctf_cli::models::Challenge {
        name: resource.name_any(),
        full_name: resource.spec.full_name.clone(),
        description: resource.spec.description.clone(),
        categories: resource.spec.categories.clone(),
        tags: resource.spec.tags.clone(),
        authors: resource.spec.authors.clone(),
        instance_duration_seconds: resource.spec.instance_duration_seconds,
        instancing: resource.spec.instancing,
        files: resource
            .spec
            .files
            .iter()
            .map(|file| match file {
                ctf_cli::k8s::ChallengeFileRef::S3 { key, .. } => PathBuf::from(key),
            })
            .collect(),
        flags: resource
            .spec
            .flags
            .iter()
            .map(|flag| ChallengeFlag {
                name: flag.name.clone(),
                label: flag.label.clone(),
                body: flag.body.clone(),
                prefix: flag.prefix.clone(),
                format: flag.format,
                random: flag.random.clone(),
                options: flag.options.clone(),
                scoring: flag.scoring.clone(),
                difficulty: flag.difficulty,
            })
            .collect(),
        containers: resource
            .spec
            .containers
            .iter()
            .map(|container| ChallengeContainer {
                name: container.name.clone(),
                image: ContainerImage::Image {
                    image: container.image.clone(),
                },
                flags: container.flags.clone(),
                ports: container.ports.clone(),
            })
            .collect(),
    };

    match core.validate_strict() {
        Ok(()) => {
            status
                .conditions
                .set("Validation", ConditionStatus::True, "Validated", "");
        }
        Err(err) => {
            status.conditions.set(
                "Validation",
                ConditionStatus::False,
                "InvalidSpec",
                err.message(),
            );
        }
    }

    if status.conditions.all_true() {
        status
            .conditions
            .set("Ready", ConditionStatus::True, "Ready", "");
    } else {
        status
            .conditions
            .set("Ready", ConditionStatus::False, "NotReady", "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_cli::k8s::{ChallengeContainer, ChallengeFileRef, ChallengeSpec};
    use std::collections::HashSet;

    fn valid_challenge() -> Challenge {
        let mut challenge = Challenge::new(
            "chall-1",
            ChallengeSpec {
                full_name: "Challenge One".to_string(),
                description: Some("desc".to_string()),
                instance_duration_seconds: None,
                instancing: true,
                categories: HashSet::new(),
                tags: HashSet::new(),
                authors: HashSet::new(),
                flags: vec![ctf_cli::models::ChallengeFlag {
                    name: "flag-1".to_string(),
                    body: "flagbody".to_string(),
                    prefix: None,
                    format: None,
                    random: None,
                    label: None,
                    options: ctf_cli::models::FlagMatchOptions::default(),
                    scoring: ctf_cli::models::FlagScoring::default(),
                    difficulty: None,
                }],
                containers: vec![ChallengeContainer {
                    name: "svc".to_string(),
                    image: "repo/svc:latest".to_string(),
                    flags: vec![ctf_cli::models::ContainerFlag {
                        name: "flag-1".to_string(),
                        env: "FLAG".to_string(),
                    }],
                    ports: vec![ctf_cli::models::ContainerPort {
                        name: "http".to_string(),
                        container_port: 8080,
                        expose: true,
                        port_type: ctf_cli::models::ChallengePortType::HTTP,
                        env: None,
                    }],
                }],
                files: vec![ChallengeFileRef::S3 {
                    bucket: "files".to_string(),
                    key: "challenge/chall-1/file.zip".to_string(),
                    endpoint: None,
                    path_style: true,
                }],
            },
        );
        challenge.metadata.namespace = Some("ctf".to_string());
        challenge
    }

    #[test]
    fn validate_spec_sets_ready_true_for_valid_spec() {
        let challenge = valid_challenge();
        let mut status = ChallengeStatus::default();

        validate_spec(&challenge, &mut status);

        assert!(status.conditions.all_true());
    }

    #[test]
    fn validate_spec_sets_ready_false_for_invalid_spec() {
        let mut challenge = valid_challenge();
        challenge.spec.flags.push(ctf_cli::models::ChallengeFlag {
            name: "flag-1".to_string(),
            body: "other".to_string(),
            prefix: None,
            format: None,
            random: None,
            label: None,
            options: ctf_cli::models::FlagMatchOptions::default(),
            scoring: ctf_cli::models::FlagScoring::default(),
            difficulty: None,
        });

        let mut status = ChallengeStatus::default();
        validate_spec(&challenge, &mut status);

        assert!(!status.conditions.all_true());
        assert!(status.conditions.get("Validation").is_some());
        assert!(status.conditions.get("Ready").is_some());
    }
}
