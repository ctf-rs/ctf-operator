use crate::v1alpha1::{
    PortClaim, PortClaimReservationStatus, PortClaimStatus, ReservationSet, ReservationSetStatus,
};
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

pub struct PortClaimController {}

impl Controller<PortClaim> for PortClaimController {
    const NAME: &'static str = "portclaim-controller";

    async fn reconcile(portclaim: Arc<PortClaim>, ctx: Arc<Context>) -> crate::Result<Action> {
        //let linked_ports = get_linked_ports(&portclaim, &ctx).await?;

        //info!("Found {} linked ports", linked_ports.len());

        let status = &portclaim.status;
        let status = match status {
            Some(status) => status,
            None => return initialize_status(&portclaim, &ctx).await,
        };

        let reservation_status = match &status.reservation {
            Some(reservation_status) => reservation_status,
            None => return initialize_status(&portclaim, &ctx).await,
        };

        if portclaim.spec.reservation.name != reservation_status.name
            || portclaim.spec.reservation.namespace != reservation_status.namespace
        {
            return reconcile_parent_changed(&portclaim, &ctx, &reservation_status).await;
        }

        let reservation = get_parent_reservation(&portclaim, &ctx).await?;
        let reservation = match reservation {
            Some(reservation) => reservation,
            None => {
                return reconcile_no_reservation(&portclaim, &ctx, status, reservation_status)
                    .await;
            }
        };

        let reserved_port = get_reserved_port(&portclaim, &ctx, &reservation).await?;
        let reserved_port = match reserved_port {
            Some(reserved_port) => reserved_port,
            None => {
                return reconcile_no_reserved_port(&portclaim, &ctx, status, reservation_status)
                    .await;
            }
        };

        if status.port != Some(reserved_port) {
            return reconcile_new_reservation_port(
                &portclaim,
                &ctx,
                status,
                reservation_status,
                &reservation,
                reserved_port,
            )
            .await;
        }

        Ok(Action::await_change())
    }
}

async fn initialize_status(portclaim: &PortClaim, ctx: &Context) -> crate::Result<Action> {
    todo!()
}

async fn reconcile_parent_changed(
    portclaim: &PortClaim,
    ctx: &Context,
    reservation_status: &PortClaimReservationStatus,
) -> crate::Result<Action> {
    todo!()
}

async fn reconcile_no_reservation(
    portclaim: &PortClaim,
    ctx: &Context,
    status: &PortClaimStatus,
    reservation_status: &PortClaimReservationStatus,
) -> crate::Result<Action> {
    todo!()
}

async fn reconcile_no_reserved_port(
    portclaim: &PortClaim,
    ctx: &Context,
    status: &PortClaimStatus,
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
    todo!()
}

async fn get_reserved_port(
    portclaim: &PortClaim,
    ctx: &Context,
    reservation: &ReservationSet,
) -> crate::Result<Option<u16>> {
    todo!()
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
