// use crate::controller::{Context, Result, upsert_condition};
// use crate::crd::v1alpha1::{
//     PortClaim, ReservationSet, ReservationSetClaimStatus, ReservationSetStatus,
// };
// use jiff::Timestamp;
// use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
// use kube::api::ListParams;
// use kube::runtime::reflector::ObjectRef;
// use kube::runtime::{WatchStreamExt, watcher};
// use rand::{Rng, rng};
// use std::collections::HashSet;
// use std::sync::Arc;
// use tracing::info;

// use kube::runtime::controller::Action;
// use kube::{Api, Client, Resource, ResourceExt};

// use crate::controller::Controller;

// pub struct ReservationSetController {}

// impl Controller<ReservationSet> for ReservationSetController {
//     const NAME: &'static str = "reservationset-controller";

//     fn kube_controller(client: Client) -> kube::runtime::Controller<ReservationSet> {
//         let reservations = Api::<ReservationSet>::all(client.clone());
//         let ports = Api::<PortClaim>::all(client.clone());

//         let ports_stream = watcher(ports, watcher::Config::default().any_semantic())
//             .modify(|pc| {
//                 pc.managed_fields_mut().clear();
//                 pc.status = None;
//                 // if let Some(status) = &mut pc.status {
//                 //     status.conditions.clear();
//                 //     status.port = None;
//                 // }
//             })
//             .touched_objects();

//         kube::runtime::Controller::new(reservations, watcher::Config::default().any_semantic())
//             .watches_stream(ports_stream, |m| {
//                 let reservation = &m.spec.reservation;
//                 let ns = m.namespace().unwrap_or_default();

//                 vec![
//                     ObjectRef::new(&reservation.name)
//                         .within(reservation.namespace.as_ref().unwrap_or(&ns)),
//                 ]
//                 // let mut refs = vec![
//                 //     ObjectRef::new(&reservation.name)
//                 //         .within(reservation.namespace.as_ref().unwrap_or(&ns)),
//                 // ];

//                 // if let Some(Some(stat_reservation)) = m.status.map(|s| s.reservation) {
//                 //     if reservation.name != stat_reservation.name
//                 //         && reservation.namespace != stat_reservation.namespace
//                 //     {
//                 //         refs.push(
//                 //             ObjectRef::new(&stat_reservation.name)
//                 //                 .within(stat_reservation.namespace.as_ref().unwrap_or(&ns)),
//                 //         );
//                 //     }
//                 // }

//                 // refs
//             })
//     }

//     async fn reconcile(
//         reservationset: Arc<ReservationSet>,
//         ctx: Arc<Context>,
//     ) -> crate::Result<Action> {
//         let mut status = get_or_create_reservation_status(&reservationset, &ctx).await?;

//         if !validate_spec(&reservationset, &ctx, &mut status).await? {
//             return Ok(Action::await_change());
//         }

//         let linked_ports = get_linked_claims(&reservationset, &ctx).await?;

//         info!("aaa");
//         prune_invalid_ports(&reservationset, &ctx, &mut status).await?;
//         info!("bbb");
//         adjust_claims(&reservationset, &ctx, &mut status, &linked_ports).await?;

//         let mut reserved_ports: HashSet<u16> =
//             status.claims.iter().filter_map(|c| c.port).collect();

//         if reserved_ports.len() != linked_ports.len() {
//             reserve_ports(&reservationset, &ctx, &mut status, &mut reserved_ports).await?;
//         }

//         Ok(Action::await_change())
//     }
// }

// async fn get_or_create_reservation_status(
//     reservation: &ReservationSet,
//     ctx: &Context,
// ) -> Result<ReservationSetStatus> {
//     match &reservation.status {
//         Some(reservation) => Ok(reservation.clone()),
//         None => {
//             let status: ReservationSetStatus = ReservationSetStatus {
//                 claims: Vec::new(),
//                 conditions: vec![Condition {
//                     last_transition_time: Timestamp::now().into(),
//                     message: "".into(),
//                     observed_generation: reservation.meta().generation,
//                     reason: "Created".into(),
//                     status: "False".into(),
//                     type_: "Validated".into(),
//                 }],
//             };

//             ReservationSetController::patch_status_namespaced(reservation, ctx, status.clone())
//                 .await?;
//             Ok(status)
//         }
//     }
// }

// async fn validate_spec(
//     reservation: &ReservationSet,
//     ctx: &Context,
//     status: &mut ReservationSetStatus,
// ) -> Result<bool> {
//     let (error_message, error_reason, validated) =
//         match (reservation.spec.start, reservation.spec.end) {
//             (start, _) if start < 1024 || start > 49151 => (
//                 "Start port must be between 1024 than 49151",
//                 "InvalidStartPort",
//                 false,
//             ),
//             (_, end) if end < 1024 || end > 49151 => (
//                 "End port must be between 1024 than 49151",
//                 "InvalidEndPort",
//                 false,
//             ),
//             (start, end) if start > end => (
//                 "Start port must be less or equal than end port",
//                 "InvalidPortOrder",
//                 false,
//             ),
//             _ => ("", "ReservationSetValid", true),
//         };

//     let condition = Condition {
//         last_transition_time: Timestamp::now().into(),
//         message: error_message.into(),
//         observed_generation: reservation.meta().generation,
//         reason: error_reason.into(),
//         status: if validated {
//             "True".into()
//         } else {
//             "False".into()
//         },
//         type_: "Validated".into(),
//     };

//     let changed = upsert_condition(&mut status.conditions, condition);

//     // No need to update timestamp for non changed valid status
//     if changed || !validated {
//         ReservationSetController::patch_status_namespaced(reservation, ctx, status.clone()).await?;
//     }

//     Ok(validated)
// }

// async fn adjust_claims(
//     reservation: &ReservationSet,
//     ctx: &Context,
//     status: &mut ReservationSetStatus,
//     claims: &[PortClaim],
// ) -> Result<()> {
//     let mut names: HashSet<_> = status
//         .claims
//         .iter()
//         .map(|c| (c.name.clone(), c.namespace.clone()))
//         .collect();

//     let mut new_claims = Vec::new();
//     for claim in claims {
//         // TODO: error on empty namespace or name
//         let ns = claim.namespace().unwrap_or_default();
//         let name = claim.name_any();
//         if !names.remove(&(name.clone(), ns.clone())) {
//             new_claims.push((name, ns));
//         }
//     }

//     if names.is_empty() && new_claims.is_empty() {
//         return Ok(());
//     }

//     status
//         .claims
//         .retain(|c| !names.contains(&(c.name.clone(), c.namespace.clone())));

//     for (name, ns) in new_claims {
//         status.claims.push(ReservationSetClaimStatus {
//             port: None,
//             name,
//             namespace: ns,
//             conditions: vec![Condition {
//                 last_transition_time: Timestamp::now().into(),
//                 message: "".into(),
//                 observed_generation: reservation.meta().generation,
//                 reason: "Found".into(),
//                 status: "False".into(),
//                 type_: "Reserved".into(),
//             }],
//         });
//     }

//     ReservationSetController::patch_status_namespaced(reservation, ctx, status.clone()).await?;
//     Ok(())
// }

// async fn prune_invalid_ports(
//     reservation: &ReservationSet,
//     ctx: &Context,
//     status: &mut ReservationSetStatus,
// ) -> Result<()> {
//     let mut ports = HashSet::new();
//     let original_size = status.claims.len();
//     status.claims.retain(|c| match c.port {
//         None => true,
//         Some(port) => {
//             (reservation.spec.start..=reservation.spec.end).contains(&port) && ports.insert(port)
//         }
//     });

//     if original_size != status.claims.len() {
//         ReservationSetController::patch_status_namespaced(reservation, ctx, status.clone()).await?;
//     }

//     Ok(())
// }

// async fn reserve_ports(
//     reservation: &ReservationSet,
//     ctx: &Context,
//     status: &mut ReservationSetStatus,
//     reserved_ports: &mut HashSet<u16>,
// ) -> Result<()> {
//     for claim in &mut status.claims {
//         if claim.port.is_none() {
//             match random_free_port(reservation.spec.start, reservation.spec.end, reserved_ports) {
//                 Some(port) => {
//                     claim.port = Some(port);
//                     upsert_condition(
//                         &mut claim.conditions,
//                         Condition {
//                             last_transition_time: Timestamp::now().into(),
//                             message: "Port is assigned".into(),
//                             observed_generation: reservation.meta().generation,
//                             reason: "PortReserved".into(),
//                             status: "True".into(),
//                             type_: "Reserved".into(),
//                         },
//                     );
//                 }
//                 None => {
//                     upsert_condition(
//                         &mut claim.conditions,
//                         Condition {
//                             last_transition_time: Timestamp::now().into(),
//                             message: "ReservationSet is full".into(),
//                             observed_generation: reservation.meta().generation,
//                             reason: "NoFreeReservation".into(),
//                             status: "False".into(),
//                             type_: "Reserved".into(),
//                         },
//                     );
//                 }
//             }
//         }
//     }

//     ReservationSetController::patch_status_namespaced(reservation, ctx, status.clone()).await?;
//     Ok(())
// }

// fn random_free_port(lo: u16, hi: u16, reserved: &HashSet<u16>) -> Option<u16> {
//     let n = (hi as u32).wrapping_sub(lo as u32) + 1;
//     if reserved.len() as u32 >= n {
//         return None;
//     }

//     let mut rng = rng();
//     // cap attempts so we don't spin forever when almost full
//     let max_tries = 32.min(n as usize);

//     for _ in 0..max_tries {
//         let p = rng.random_range(lo..=hi);
//         if !reserved.contains(&p) {
//             return Some(p);
//         }
//     }

//     // fallback: deterministic scan from a random start (guaranteed if any free)
//     let start = rng.random_range(lo..=hi);
//     for i in 0..n {
//         let p = lo + (((start - lo) as u32 + i) % n) as u16;
//         if !reserved.contains(&p) {
//             return Some(p);
//         }
//     }
//     None
// }

// async fn get_linked_claims(
//     reservationset: &ReservationSet,
//     ctx: &Context,
// ) -> crate::Result<Vec<PortClaim>> {
//     let client = ctx.client.clone();
//     let ports = Api::<PortClaim>::all(client.clone());

//     let name: String = reservationset.name_any();
//     let namespace = reservationset.namespace().unwrap_or_default();

//     // TODO: Add protections if namespace is borken
//     debug_assert!(!namespace.is_empty());

//     let mut ports_no_ns = ports
//         .list(
//             &ListParams::default()
//                 .fields(&format!(
//                     "metadata.namespace={namespace},spec.reservation.name={name},spec.reservation.namespace="
//                 )),
//         )
//         .await?;

//     let mut ports_ns = ports
//         .list(&ListParams::default().fields(&format!(
//             "spec.reservation.name={name},spec.reservation.namespace={namespace}"
//         )))
//         .await?;

//     let mut ports = Vec::<PortClaim>::new();

//     ports.append(&mut ports_no_ns.items);
//     ports.append(&mut ports_ns.items);

//     Ok(ports)
// }
