use clap::Parser;
use ctf_core::k8s::{Challenge, Ctf, Flag, Instance, PortClaim, ReservationSet};
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
        Command::Crd(crd_args) => gen_crd(&crd_args)?,
        Command::Operator(operator_args) => start_operator(operator_args).await?,
    }

    Ok(())
}

async fn start_operator(args: OperatorArgs) -> anyhow::Result<()> {
    runtime::run(args.namespaces, args.instance_namespace_prefix, args.config).await?;
    Ok(())
}

fn gen_crd(args: &CrdArgs) -> anyhow::Result<()> {
    std::fs::create_dir_all(&args.output)?;
    for (name, crd) in [
        ("ctfs.yaml", Ctf::crd()),
        ("challenges.yaml", Challenge::crd()),
        ("flags.yaml", Flag::crd()),
        ("instances.yaml", Instance::crd()),
        ("reservationsets.yaml", ReservationSet::crd()),
        ("portclaims.yaml", PortClaim::crd()),
    ] {
        std::fs::write(args.output.join(name), serde_saphyr::to_string(&crd)?)?;
    }
    Ok(())
}
