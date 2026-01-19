use crate::v1alpha1::{
    PortClaim, PortClaimReservationStatus, PortClaimStatus, ReservationSet, ReservationSetStatus,
};
use crate::{Context, Result, upsert_condition};
use jiff::Timestamp;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Status};
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

pub struct PortClaimController {}

impl Controller<PortClaim> for PortClaimController {
    const NAME: &'static str = "portclaim-controller";

    async fn reconcile(portclaim: Arc<PortClaim>, ctx: Arc<Context>) -> crate::Result<Action> {
        //let linked_ports = get_linked_ports(&portclaim, &ctx).await?;

        //info!("Found {} linked ports", linked_ports.len());

        let status = portclaim.status.clone();
        let mut status = match status {
            Some(status) => status,
            None => return initialize_status(&portclaim, &ctx).await,
        };

        let reservation_status = match status.reservation.clone() {
            Some(reservation_status) => reservation_status,
            None => return initialize_status(&portclaim, &ctx).await,
        };

        if portclaim.spec.reservation.name != reservation_status.name
            || portclaim.spec.reservation.namespace != reservation_status.namespace
        {
            return reconcile_parent_changed(&portclaim, &ctx, &reservation_status).await;
        }

        let reservation = get_parent_reservation(&portclaim, &ctx).await?;
        update_reservation_found_condition(&portclaim, &ctx, &mut status, &reservation).await?;
        let reservation = match reservation {
            Some(reservation) => reservation,
            None => return Ok(Action::requeue(Duration::from_secs(10))),
        };

        let (reserved_port, message) = get_reserved_port(&portclaim, &ctx, &reservation).await?;
        update_reservation_port_condition(
            &portclaim,
            &ctx,
            &mut status,
            &reservation,
            reserved_port,
            message,
        )
        .await?;
        let reserved_port = match reserved_port {
            Some(reserved_port) => reserved_port,
            None => return Ok(Action::requeue(Duration::from_secs(10))),
        };

        // if status.port != Some(reserved_port) {
        //     return reconcile_new_reservation_port(
        //         &portclaim,
        //         &ctx,
        //         &status,
        //         reservation_status,
        //         &reservation,
        //         reserved_port,
        //     )
        //     .await;
        // }

        Ok(Action::await_change())
    }
}

async fn initialize_status(portclaim: &PortClaim, ctx: &Context) -> crate::Result<Action> {
    let mut status = portclaim.status.clone().unwrap_or_else(|| PortClaimStatus {
        port: None,
        reservation: None,
        conditions: Vec::new(),
    });

    let portclaim_gen = portclaim.meta().generation;
    status.port = None;
    status.reservation = Some(PortClaimReservationStatus {
        name: portclaim.spec.reservation.name.clone(),
        namespace: portclaim.spec.reservation.namespace.clone(),
    });

    upsert_condition(
        &mut status.conditions,
        Condition {
            last_transition_time: Timestamp::now().into(),
            message: "".into(),
            observed_generation: portclaim_gen,
            reason: "ReservationSetFound".into(),
            status: "False".into(),
            type_: "Allocated".into(),
        },
    );

    PortClaimController::patch_status_namespaced(portclaim, ctx, status).await?;
    return Ok(Action::requeue(Duration::from_secs(5)));
}

async fn update_reservation_found_condition(
    portclaim: &PortClaim,
    ctx: &Context,
    status: &mut PortClaimStatus,
    reservation: &Option<ReservationSet>,
) -> crate::Result<()> {
    let portclaim_gen = portclaim.meta().generation;

    let condition = Condition {
        last_transition_time: Timestamp::now().into(),
        message: if reservation.is_some() {
            "Reservation set found".into()
        } else {
            "Reservation set not found".into()
        },
        observed_generation: portclaim_gen,
        reason: "ReservationSetFound".into(),
        status: if reservation.is_some() {
            "True".into()
        } else {
            "False".into()
        },
        type_: "Allocated".into(),
    };

    upsert_condition(&mut status.conditions, condition);

    PortClaimController::patch_status_namespaced(portclaim, ctx, status).await?;

    Ok(())
}

async fn update_reservation_port_condition(
    portclaim: &PortClaim,
    ctx: &Context,
    status: &mut PortClaimStatus,
    reservation: &ReservationSet,
    port: Option<u16>,
    message: &str,
) -> crate::Result<()> {
    let portclaim_gen = portclaim.meta().generation;

    let condition = Condition {
        last_transition_time: Timestamp::now().into(),
        message: message.into(),
        observed_generation: portclaim_gen,
        reason: "PortAllocated".into(),
        status: if port.is_some() {
            "True".into()
        } else {
            "False".into()
        },
        type_: "Allocated".into(),
    };

    upsert_condition(&mut status.conditions, condition);

    PortClaimController::patch_status_namespaced(portclaim, ctx, status).await?;
    Ok(())
}

async fn reconcile_parent_changed(
    portclaim: &PortClaim,
    ctx: &Context,
    reservation_status: &PortClaimReservationStatus,
) -> crate::Result<Action> {
    todo!()
}

async fn reconcile_new_reservation_port(
    portclaim: &PortClaim,
    ctx: &Context,
    status: &PortClaimStatus,
    reservation_status: &PortClaimReservationStatus,
    reservation: &ReservationSet,
    port: u16,
) -> crate::Result<Action> {
    todo!()
}

async fn get_parent_reservation(
    portclaim: &PortClaim,
    ctx: &Context,
) -> crate::Result<Option<ReservationSet>> {
    let client = ctx.client.clone();
    let ns = portclaim.namespace().unwrap_or_default();

    let reservations = Api::<ReservationSet>::namespaced(
        client,
        &portclaim.spec.reservation.namespace.as_ref().unwrap_or(&ns),
    );

    let reservation = reservations
        .get_opt(&portclaim.spec.reservation.name)
        .await?;

    Ok(reservation)
}

async fn get_reserved_port(
    portclaim: &PortClaim,
    ctx: &Context,
    reservation: &ReservationSet,
) -> crate::Result<(Option<u16>, &'static str)> {
    match &reservation.status {
        None => Ok((None, "ReservationSet status not populated")),
        Some(status) => {
            let claim = status.claims.iter().find(|s| {
                s.name == portclaim.name_any()
                    && s.namespace == portclaim.namespace().unwrap_or_default()
            });
            match claim {
                None => Ok((None, "ReservationSet does not have initialized claim")),
                Some(claim) => match claim.port {
                    None => Ok((None, "Port is not allocated")),
                    Some(port) => Ok((Some(port), "Port is allocated")),
                },
            }
        }
    }
}

// async fn get_linked_ports(portclaim: &PortClaim, ctx: &Context) -> crate::Result<Vec<PortClaim>> {
//     let client = ctx.client.clone();
//     let ports = Api::<PortClaim>::all(client.clone());

//     let name: String = portclaim.name_any();
//     let namespace = portclaim.namespace().unwrap_or_default();

//     // TODO: Add protections if namespace is borken
//     debug_assert!(!namespace.is_empty());

//     let mut ports_no_ns = ports
//         .list(
//             &ListParams::default()
//                 .fields(&format!(
//                     "metadata.namespace={namespace},spec.reservationRef.name={name},spec.reservationRef.namespace="
//                 )),
//         )
//         .await?;

//     let mut ports_ns = ports
//         .list(&ListParams::default().fields(&format!(
//             "spec.reservationRef.name={name},spec.reservationRef.namespace={namespace}"
//         )))
//         .await?;

//     let mut ports = Vec::<PortClaim>::new();

//     //info!("Found {} linked ports no ns", ports_no_ns.items.len());
//     ports.append(&mut ports_no_ns.items);
//     //info!("Found {} linked ports ns", ports_ns.items.len());
//     ports.append(&mut ports_ns.items);

//     //TODO: Check for duplicates (can happen if namespace was empty)

//     Ok(ports)
// }
