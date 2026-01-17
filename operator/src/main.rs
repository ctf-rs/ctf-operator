use clap::Parser;
use ctf_operator::{Args, Controller, FlagController, State, init_telemetry, v1alpha1::Flag};
use kube::CustomResourceExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    //gen_crd();
    start_operator().await?;
    Ok(())
}

async fn start_operator() -> anyhow::Result<()> {
    init_telemetry().await;

    let state = State::new();

    let flag_controller = FlagController {};

    FlagController::run(state.clone()).await;

    //tokio::join!(FlagController::run(state.clone()), ..).1?;
    Ok(())
}

fn gen_crd() {
    let crd = Flag::crd();
    std::fs::remove_dir_all("crd").unwrap();
    std::fs::create_dir_all("crd").unwrap();
    std::fs::write("crd/flags.yaml", serde_yaml::to_string(&crd).unwrap()).unwrap();
}
