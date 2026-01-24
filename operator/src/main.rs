use clap::Parser;
use ctf_operator::{
    Args, Command, CrdArgs, OperatorArgs, State,
    controller::{Controller, FlagController, PortClaimController, ReservationSetController},
    crd::v1alpha1::{Flag, PortClaim, ReservationSet},
    init_telemetry,
};
use kube::CustomResourceExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.cmd {
        Command::Crd(args) => gen_crd(args),
        Command::Operator(args) => start_operator(args).await?,
    }

    Ok(())
}

async fn start_operator(_args: OperatorArgs) -> anyhow::Result<()> {
    init_telemetry().await;

    let state = State::new();

    let flag_controller = FlagController::run(state.clone());
    let reservation_controller = ReservationSetController::run(state.clone());
    let portclaim_controller = PortClaimController::run(state.clone());

    tokio::join!(
        flag_controller,
        reservation_controller,
        portclaim_controller
    );
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
}
