use clap::Parser;
use ctf_operator::{
    Args, Command, Controller, CrdArgs, FlagController, OperatorArgs, State, init_telemetry,
    v1alpha1::Flag,
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

    FlagController::run(state.clone()).await;

    //tokio::join!(FlagController::run(state.clone()), ..).1?;
    Ok(())
}

fn gen_crd(_args: CrdArgs) {
    let crd = Flag::crd();
    std::fs::remove_dir_all("crd").unwrap();
    std::fs::create_dir_all("crd").unwrap();
    std::fs::write("crd/flags.yaml", serde_yaml::to_string(&crd).unwrap()).unwrap();
}
