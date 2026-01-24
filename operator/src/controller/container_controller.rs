use crate::controller::{Context, Result, upsert_condition};
use crate::crd::v1alpha1::{Container, ContainerStatus, Flag};
use jiff::Timestamp;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::runtime::{WatchStreamExt, watcher};
use rand::Rng;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

use kube::core::object::HasStatus;
use kube::runtime::controller::Action;
use kube::{Api, Client, Resource, ResourceExt};

use crate::controller::Controller;

pub struct ContainerController {}

impl Controller<Container> for ContainerController {
    const NAME: &'static str = "container-controller";

    // fn kube_controller(client: Client) -> kube::runtime::Controller<Container> {
    //     let containers = Api::<Container>::all(client.clone());
    //     let flags = Api::<Flag>::all(client.clone());

    //     let ports_stream = watcher(flags, watcher::Config::default().any_semantic())
    //         .modify(|rs| {
    //             rs.managed_fields_mut().clear();
    //             if let Some(status) = &mut rs.status {
    //                 for condition in &mut status.conditions {
    //                     // Ignore same refresh
    //                     condition.last_transition_time = Timestamp::UNIX_EPOCH.into()
    //                 }
    //             }
    //         })
    //         .touched_objects();

    //     kube::runtime::Controller::new(containers, watcher::Config::default().any_semantic())
    //         .owns_with(api, dyntype, wc)
    //         .watches_stream(ports_stream, |m| {
    //             let mut refs = Vec::new();

    //             // if let Some(status) = &m.status {
    //             //     for claim in &status.claims {
    //             //         refs.push(ObjectRef::new(&claim.name).within(&claim.namespace));
    //             //     }
    //             // }

    //             refs
    //         })
    // }

    async fn reconcile(flag: Arc<Container>, ctx: Arc<Context>) -> Result<Action> {
        Ok(Action::await_change())
    }
}
