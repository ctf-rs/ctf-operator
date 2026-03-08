use clap::Parser;

use ctf_operator::crd::v1alpha1::{Challenge, Flag, Instance, Jeopardy, PortClaim, ReservationSet};
use ctf_operator::{init_telemetry, runtime, Args, Command, CrdArgs, OperatorArgs};
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

    runtime::run().await?;

    Ok(())
}

fn gen_crd(_args: CrdArgs) {
    std::fs::remove_dir_all("crd").unwrap();
    std::fs::create_dir_all("crd").unwrap();
    std::fs::write(
        "crd/flags.yaml",
        serde_saphyr::to_string(&Flag::crd()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        "crd/jeopardy.yaml",
        serde_saphyr::to_string(&Jeopardy::crd()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        "crd/reservationsets.yaml",
        serde_saphyr::to_string(&ReservationSet::crd()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        "crd/portclaims.yaml",
        serde_saphyr::to_string(&PortClaim::crd()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        "crd/challenges.yaml",
        serde_saphyr::to_string(&Challenge::crd()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        "crd/instances.yaml",
        serde_saphyr::to_string(&Instance::crd()).unwrap(),
    )
    .unwrap();
}
