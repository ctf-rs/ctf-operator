use clap::Parser;
use ctf_k8s::{Challenge, Ctf, Flag, Instance};
use ctf_operator::{runtime, Args, Command, CrdArgs, OperatorArgs};
use kube::CustomResourceExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    match args.cmd {
        Command::Crd(crd_args) => gen_crd(crd_args)?,
        Command::Operator(operator_args) => start_operator(operator_args).await?,
    }

    Ok(())
}

async fn start_operator(args: OperatorArgs) -> anyhow::Result<()> {
    runtime::run(args.namespaces).await?;
    Ok(())
}

fn gen_crd(_args: CrdArgs) -> anyhow::Result<()> {
    std::fs::remove_dir_all("crd").ok();
    std::fs::create_dir_all("crd")?;

    std::fs::write("crd/ctfs.yaml", serde_yaml::to_string(&Ctf::crd())?)?;
    std::fs::write(
        "crd/challenges.yaml",
        serde_yaml::to_string(&Challenge::crd())?,
    )?;
    std::fs::write("crd/flags.yaml", serde_yaml::to_string(&Flag::crd())?)?;
    std::fs::write(
        "crd/instances.yaml",
        serde_yaml::to_string(&Instance::crd())?,
    )?;

    Ok(())
}
