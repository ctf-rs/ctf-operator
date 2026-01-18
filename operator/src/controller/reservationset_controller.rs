use crate::v1alpha1::{PortClaim, ReservationSet, ReservationSetStatus};
use crate::{Context, Result, upsert_condition};
use jiff::Timestamp;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::api::ListParams;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::watcher;
use rand::Rng;
use std::sync::Arc;
use std::time::Duration;
use tracing::instrument::WithSubscriber;
use tracing::{info, trace};

use kube::core::object::HasStatus;
use kube::runtime::controller::Action;
use kube::{Api, Client, Resource, ResourceExt};

use crate::Controller;

pub struct ReservationSetController {}

impl Controller<ReservationSet> for ReservationSetController {
    const NAME: &'static str = "reservationset-controller";

    fn kube_controller(client: Client) -> kube::runtime::Controller<ReservationSet> {
        let reservations = Api::<ReservationSet>::all(client.clone());
        let ports = Api::<PortClaim>::all(client.clone());
        kube::runtime::Controller::new(reservations, watcher::Config::default().any_semantic())
            .watches(ports, watcher::Config::default().any_semantic(), |m| {
                let reservation = &m.spec.reservation;
                let ns = m.namespace().unwrap_or_default();
                let mut refs = vec![
                    ObjectRef::new(&reservation.name)
                        .within(reservation.namespace.as_ref().unwrap_or(&ns)),
                ];

                if let Some(Some(stat_reservation)) = m.status.map(|s| s.reservation) {
                    if reservation.name != stat_reservation.name
                        && reservation.namespace != stat_reservation.namespace
                    {
                        refs.push(
                            ObjectRef::new(&stat_reservation.name)
                                .within(stat_reservation.namespace.as_ref().unwrap_or(&ns)),
                        );
                    }
                }

                refs
            })
    }

    async fn reconcile(
        reservationset: Arc<ReservationSet>,
        ctx: Arc<Context>,
    ) -> crate::Result<Action> {
        let linked_ports = get_linked_ports(&reservationset, &ctx).await?;

        info!("Found {} linked ports", linked_ports.len());

        Ok(Action::requeue(Duration::from_mins(1)))
    }
}

async fn get_linked_ports(
    reservationset: &ReservationSet,
    ctx: &Context,
) -> crate::Result<Vec<PortClaim>> {
    let client = ctx.client.clone();
    let ports = Api::<PortClaim>::all(client.clone());

    let name: String = reservationset.name_any();
    let namespace = reservationset.namespace().unwrap_or_default();

    // TODO: Add protections if namespace is borken
    debug_assert!(!namespace.is_empty());

    let mut ports_no_ns = ports
        .list(
            &ListParams::default()
                .fields(&format!(
                    "metadata.namespace={namespace},spec.reservationRef.name={name},spec.reservationRef.namespace="
                )),
        )
        .await?;

    let mut ports_ns = ports
        .list(&ListParams::default().fields(&format!(
            "spec.reservationRef.name={name},spec.reservationRef.namespace={namespace}"
        )))
        .await?;

    let mut ports = Vec::<PortClaim>::new();

    //info!("Found {} linked ports no ns", ports_no_ns.items.len());
    ports.append(&mut ports_no_ns.items);
    //info!("Found {} linked ports ns", ports_ns.items.len());
    ports.append(&mut ports_ns.items);

    //TODO: Check for duplicates (can happen if namespace was empty)

    Ok(ports)
}
