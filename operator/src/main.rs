use clap::Parser;
use ctf_cli::k8s::{Challenge, Ctf, Flag, Instance, PortClaim, ReservationSet};
use ctf_operator::{runtime, Args, Command, CrdArgs, OperatorArgs};
use kube::CustomResourceExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

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
    runtime::run(args.namespaces, args.instance_namespace_prefix, args.config).await?;
    Ok(())
}

fn gen_crd(_args: CrdArgs) -> anyhow::Result<()> {
    let _ = std::fs::remove_dir_all("crd");
    std::fs::create_dir_all("crd")?;

    std::fs::write("crd/ctfs.yaml", serde_saphyr::to_string(&Ctf::crd())?)?;
    std::fs::write(
        "crd/challenges.yaml",
        serde_saphyr::to_string(&Challenge::crd())?,
    )?;
    std::fs::write("crd/flags.yaml", serde_saphyr::to_string(&Flag::crd())?)?;
    std::fs::write(
        "crd/instances.yaml",
        serde_saphyr::to_string(&Instance::crd())?,
    )?;
    std::fs::write(
        "crd/reservationsets.yaml",
        serde_saphyr::to_string(&ReservationSet::crd())?,
    )?;
    std::fs::write(
        "crd/portclaims.yaml",
        serde_saphyr::to_string(&PortClaim::crd())?,
    )?;

    Ok(())
}
