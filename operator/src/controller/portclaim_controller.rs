use crate::v1alpha1::{PortClaim, PortClaimStatus, ReservationSet};
use crate::{Context, Result, upsert_condition};
use jiff::Timestamp;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::runtime::controller::Action;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::{WatchStreamExt, watcher};
use kube::{Api, Client, Resource, ResourceExt};
use std::sync::Arc;

use crate::Controller;

pub struct PortClaimController {}

impl Controller<PortClaim> for PortClaimController {
    const NAME: &'static str = "portclaim-controller";

    fn kube_controller(client: Client) -> kube::runtime::Controller<PortClaim> {
        let reservations = Api::<PortClaim>::all(client.clone());
        let ports = Api::<ReservationSet>::all(client.clone());

        let ports_stream = watcher(ports, watcher::Config::default().any_semantic())
            .modify(|rs| {
                rs.managed_fields_mut().clear();
                if let Some(status) = &mut rs.status {
                    for condition in &mut status.conditions {
                        // Ignore same refresh
                        condition.last_transition_time = Timestamp::UNIX_EPOCH.into()
                    }
                }
            })
            .touched_objects();

        kube::runtime::Controller::new(reservations, watcher::Config::default().any_semantic())
            .watches_stream(ports_stream, |m| {
                let mut refs = Vec::new();

                if let Some(status) = &m.status {
                    for claim in &status.claims {
                        refs.push(ObjectRef::new(&claim.name).within(&claim.namespace));
                    }
                }

                refs
            })
    }

    async fn reconcile(portclaim: Arc<PortClaim>, ctx: Arc<Context>) -> crate::Result<Action> {
        let mut status = get_or_create_reservation_status(&portclaim, &ctx).await?;

        let reservationset = match get_reservationset(&portclaim, &ctx, &mut status).await? {
            Some(reservationset) => reservationset,
            None => return Ok(Action::await_change()),
        };

        let _ = get_reserved_port(&portclaim, &ctx, &mut status, &reservationset).await?;

        Ok(Action::await_change())
    }
}

async fn get_or_create_reservation_status(
    reservation: &PortClaim,
    ctx: &Context,
) -> Result<PortClaimStatus> {
    match &reservation.status {
        Some(reservation) => Ok(reservation.clone()),
        None => {
            let status: PortClaimStatus = PortClaimStatus {
                port: None,
                conditions: vec![Condition {
                    last_transition_time: Timestamp::now().into(),
                    message: "".into(),
                    observed_generation: reservation.meta().generation,
                    reason: "Created".into(),
                    status: "False".into(),
                    type_: "Reserved".into(),
                }],
            };

            PortClaimController::patch_status_namespaced(reservation, ctx, status.clone()).await?;
            Ok(status)
        }
    }
}

async fn get_reservationset(
    portclaim: &PortClaim,
    ctx: &Context,
    status: &mut PortClaimStatus,
) -> crate::Result<Option<ReservationSet>> {
    let client = ctx.client.clone();

    let reservations = if let Some(ns) = &portclaim.spec.reservation.namespace {
        Api::<ReservationSet>::namespaced(client, &ns)
    } else {
        Api::<ReservationSet>::namespaced(client, &portclaim.namespace().unwrap_or_default())
    };

    let reservationset = reservations
        .get_opt(&portclaim.spec.reservation.name)
        .await?;

    if reservationset.is_none() {
        status.port = None;
        upsert_condition(
            &mut status.conditions,
            Condition {
                last_transition_time: Timestamp::now().into(),
                message: "Reservation set not found".into(),
                observed_generation: portclaim.meta().generation,
                reason: "ReservationSetFound".into(),
                status: "False".into(),
                type_: "Reserved".into(),
            },
        );

        PortClaimController::patch_status_namespaced(portclaim, ctx, status.clone()).await?;
    }

    Ok(reservationset)
}

async fn get_reserved_port(
    portclaim: &PortClaim,
    ctx: &Context,
    status: &mut PortClaimStatus,
    reservation: &ReservationSet,
) -> crate::Result<Option<u16>> {
    let reservationset_status = match &reservation.status {
        Some(reservationset_status) => reservationset_status,
        None => {
            status.port = None;
            upsert_condition(
                &mut status.conditions,
                Condition {
                    last_transition_time: Timestamp::now().into(),
                    message: "Reservation set does not have status".into(),
                    observed_generation: portclaim.meta().generation,
                    reason: "ReservationSetNoStatus".into(),
                    status: "False".into(),
                    type_: "Reserved".into(),
                },
            );

            PortClaimController::patch_status_namespaced(portclaim, ctx, status.clone()).await?;
            return Ok(None);
        }
    };

    let claim_status = match reservationset_status
        .claims
        .iter()
        .find(|c| c.name == portclaim.name_any())
    {
        Some(claim_status) => claim_status,
        None => {
            status.port = None;
            upsert_condition(
                &mut status.conditions,
                Condition {
                    last_transition_time: Timestamp::now().into(),
                    message: "Reservation set does not have this claim in status".into(),
                    observed_generation: portclaim.meta().generation,
                    reason: "ClaimNotFound".into(),
                    status: "False".into(),
                    type_: "Reserved".into(),
                },
            );

            PortClaimController::patch_status_namespaced(portclaim, ctx, status.clone()).await?;
            return Ok(None);
        }
    };

    let port = claim_status.port;

    match port {
        Some(port) => {
            if status.port != Some(port) {
                if status.port.is_none() {
                    upsert_condition(
                        &mut status.conditions,
                        Condition {
                            last_transition_time: Timestamp::now().into(),
                            message: "Port is allocated".into(),
                            observed_generation: portclaim.meta().generation,
                            reason: "PortAllocated".into(),
                            status: "True".into(),
                            type_: "Reserved".into(),
                        },
                    );
                } else {
                    upsert_condition(
                        &mut status.conditions,
                        Condition {
                            last_transition_time: Timestamp::now().into(),
                            message: "Port is updated".into(),
                            observed_generation: portclaim.meta().generation,
                            reason: "PortUpdated".into(),
                            status: "True".into(),
                            type_: "Reserved".into(),
                        },
                    );
                }

                status.port = Some(port);
                PortClaimController::patch_status_namespaced(portclaim, ctx, status.clone())
                    .await?;
            }

            Ok(Some(port))
        }
        None => {
            status.port = None;
            upsert_condition(
                &mut status.conditions,
                Condition {
                    last_transition_time: Timestamp::now().into(),
                    message: "Reservation set did not allocate port for this claim".into(),
                    observed_generation: portclaim.meta().generation,
                    reason: "PortAllocated".into(),
                    status: "False".into(),
                    type_: "Reserved".into(),
                },
            );

            PortClaimController::patch_status_namespaced(portclaim, ctx, status.clone()).await?;

            Ok(None)
        }
    }
}
