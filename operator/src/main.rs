use std::sync::Arc;

use clap::Parser;
use ctf_operator::{
    Args, Command, CrdArgs, OperatorArgs, State,
    controller::{Controller, FlagController, PortClaimController, ReservationSetController},
    crd::v1alpha1::{Container, Flag, PortClaim, ReservationSet},
    indexer::{Index, WatchStreamIndexExt, indexer},
    init_telemetry,
};
use futures::{StreamExt, TryStreamExt, lock::Mutex};
use k8s_openapi::api::core::v1::{Pod, Service};
use kube::{
    Client, CustomResourceExt, ResourceExt,
    runtime::{WatchStreamExt, reflector, watcher},
};

use parking_lot::RwLock;
use stream_broadcast::StreamBroadcastExt;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.cmd {
        Command::Crd(args) => gen_crd(args),
        Command::Operator(args) => start_operator(args).await?,
    }

    Ok(())
}

#[derive(Debug)]
pub struct ServiceIndex {
    pub val: Arc<RwLock<u32>>,
}

impl Index<Container> for ServiceIndex {
    fn apply(&self, obj: Container) {
        info!("apply");
    }

    fn delete(&self, obj: &Container) {
        info!("del");
    }

    fn rehydrate(&self, objs: Vec<Container>) {
        info!("rehydrate");
    }
}

impl Index<PortClaim> for ServiceIndex {
    fn apply(&self, obj: PortClaim) {
        info!("apply");
    }

    fn delete(&self, obj: &PortClaim) {
        info!("del");
    }

    fn rehydrate(&self, objs: Vec<PortClaim>) {
        info!("rehydrate");
    }
}

async fn start_operator(_args: OperatorArgs) -> anyhow::Result<()> {
    init_telemetry().await;

    let state = State::new();

    let client = Client::try_default()
        .await
        .expect("failed to create kube Client");

    let subscriber_buf_sz = 100;
    let (container_reader, container_writer) =
        reflector::store_shared::<Container>(subscriber_buf_sz);
    let (port_reader, port_writer) = reflector::store_shared::<PortClaim>(subscriber_buf_sz);

    let idx = Arc::new(ServiceIndex {
        val: Arc::new(RwLock::new(123)),
    });

    let container_controller_stream = container_writer.subscribe().unwrap();
    let port_controller_stream = port_writer.subscribe().unwrap();
    let port_watches_stream = port_writer.subscribe().unwrap();

    let services: kube::Api<Container> = kube::Api::namespaced(client.clone(), "default");
    let container_stream = kube::runtime::watcher(services, watcher::Config::default())
        .index(idx.as_ref())
        .reflect_shared(container_writer)
        .touched_objects()
        .for_each(|res| async move {
            match res {
                Ok(o) => info!("saw in root stream {}", o.name_any()),
                Err(e) => warn!("watcher error in root stream: {}", e),
            }
        });

    let pods: kube::Api<PortClaim> = kube::Api::namespaced(client.clone(), "default");
    let port_stream = kube::runtime::watcher(pods, watcher::Config::default())
        .index(idx.as_ref())
        .reflect_shared(port_writer)
        .touched_objects()
        .for_each(|res| async move {
            match res {
                Ok(o) => info!("saw in root stream {}", o.name_any()),
                Err(e) => warn!("watcher error in root stream: {}", e),
            }
        });

    // let port_controller =
    //     kube::runtime::Controller::for_shared_stream(port_controller_stream, port_reader);

    // let idx = idx.clone();
    // let container_controller =
    //     kube::runtime::Controller::for_shared_stream(container_controller_stream, container_reader)
    //         .watches_shared_stream(port_watches_stream, move |m| {
    //             let a = idx.val.read();
    //             vec![]
    //         })
    //         .run(|| {}, error_policy, context);

    tokio::join!(
        port_stream,
        container_stream,
        //port_controller,
        //container_controller
    );

    //let b = stream.index();

    //let index = indexer(c.clone(), stream);

    //.index(c)
    //.touched_objects()
    //.map(|res| res.map(Arc::new).map_err(Arc::new));

    //let subscriber = writer.subscribe().unwrap();

    //.broadcast(1);
    // .try_for_each(|p| async move {
    //     println!("Applied: {}", p.name_any());
    //     Ok(())
    // });
    // .try_for_each(|p| async move {
    //     println!("Applied: {}", p.name_any());
    //     Ok(())
    // })
    // .await?;

    // let flag_controller = FlagController::run(state.clone());
    // let reservation_controller = ReservationSetController::run(state.clone());
    // let portclaim_controller = PortClaimController::run(state.clone());

    // tokio::join!(
    //     flag_controller,
    //     reservation_controller,
    //     portclaim_controller
    // );
    Ok(())
}

fn gen_crd(_args: CrdArgs) {
    std::fs::remove_dir_all("crd").unwrap();
    std::fs::create_dir_all("crd").unwrap();
    std::fs::write(
        "crd/flags.yaml",
        serde_yaml::to_string(&Flag::crd()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        "crd/reservationsets.yaml",
        serde_yaml::to_string(&ReservationSet::crd()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        "crd/portclaims.yaml",
        serde_yaml::to_string(&PortClaim::crd()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        "crd/containers.yaml",
        serde_yaml::to_string(&Container::crd()).unwrap(),
    )
    .unwrap();
}
