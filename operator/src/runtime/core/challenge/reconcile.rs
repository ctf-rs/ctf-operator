use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_cli::k8s::{Challenge, ChallengeStatus, ConditionStatus};
use ctf_cli::models::{ChallengeContainer, ChallengeFlag, ContainerImage};
use kube::{runtime::controller::Action, ResourceExt};

pub struct ChallengeReconciler;

impl Reconciler for ChallengeReconciler {
    const NAME: &'static str = "challenge-controller";
    type K = Challenge;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<Challenge>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = obj.namespace().unwrap_or_default();
        if !state.watches_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let mut status = obj.status.clone().unwrap_or_default();
        validate_spec(&obj, &mut status);

        let ready = status.conditions.all_true();

        if obj.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(obj.as_ref(), state.client.clone(), status).await?;
        }

        if ready {
            Ok(Action::await_change())
        } else {
            Ok(Action::requeue(Duration::from_secs(
                Self::ERROR_RECONCILE_REQUEUE_TIME,
            )))
        }
    }
}

fn validate_spec(resource: &Challenge, status: &mut ChallengeStatus) {
    let core = ctf_cli::models::Challenge {
        name: resource.name_any(),
        full_name: resource.spec.full_name.clone(),
        description: resource.spec.description.clone(),
        categories: resource.spec.categories.clone(),
        difficulty: resource.spec.difficulty,
        tags: resource.spec.tags.clone(),
        authors: resource.spec.authors.clone(),
        instance_duration_seconds: resource.spec.instance_duration_seconds,
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
                categories: HashSet::new(),
                tags: HashSet::new(),
                authors: HashSet::new(),
                conditions: Default::default(),
                flags: vec![ctf_cli::models::ChallengeFlag {
                    name: "flag-1".to_string(),
                    body: "flagbody".to_string(),
                    prefix: None,
                    format: None,
                    random: None,
                    options: Default::default(),
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
            options: Default::default(),
        });

        let mut status = ChallengeStatus::default();
        validate_spec(&challenge, &mut status);

        assert!(!status.conditions.all_true());
        assert!(status.conditions.get("Validation").is_some());
        assert!(status.conditions.get("Ready").is_some());
    }
}
