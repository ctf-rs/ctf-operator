use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_core::{ChallengeContainer, ChallengeFlag, ContainerImage};
use ctf_k8s::{Challenge, ChallengeStatus, ConditionStatus};
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
    let core = ctf_core::Challenge {
        name: resource.name_any(),
        full_name: resource.spec.full_name.clone(),
        description: resource.spec.description.clone(),
        categories: resource.spec.categories.clone(),
        tags: resource.spec.tags.clone(),
        authors: resource.spec.authors.clone(),
        files: resource
            .spec
            .files
            .iter()
            .map(|file| match file {
                ctf_k8s::ChallengeFileRef::S3 { key, .. } => PathBuf::from(key),
            })
            .collect(),
        flags: resource
            .spec
            .flags
            .iter()
            .map(|flag| ChallengeFlag {
                name: flag.name.clone(),
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
