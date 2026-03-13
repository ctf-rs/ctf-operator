use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[clap(about = "CTF Operator")]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Build(BuildArgs),
    Upload(UploadArgs),
    Cluster(ClusterArgs),
}

#[derive(Debug, Parser)]
pub struct ClusterArgs {
    #[command(subcommand)]
    pub cmd: ClusterCommand,
}

#[derive(Debug, Subcommand)]
pub enum ClusterCommand {
    Create(ClusterCreateArgs),
}

#[derive(Debug, Parser)]
pub struct ClusterCreateArgs {
    #[arg(index = 1)]
    pub ctf: PathBuf,

    #[arg(
        short = 'n',
        long = "namespace",
        value_delimiter = ',',
        default_value = "ctf"
    )]
    pub namespaces: Vec<String>,

    #[arg(long = "instance-namespace-prefix", default_value = "ctf-inst-")]
    pub instance_namespace_prefix: String,

    #[arg(long, value_enum, default_value_t = ClusterBackend::Kind)]
    pub backend: ClusterBackend,

    #[arg(long = "kind-provider", value_enum, default_value_t = KindProvider::Auto)]
    pub kind_provider: KindProvider,

    #[arg(long = "operator-namespace", default_value = "ctf-operator")]
    pub operator_namespace: String,

    #[arg(long = "operator-replicas", default_value_t = 1)]
    pub operator_replicas: u16,

    #[arg(long = "no-operator", default_value_t = false)]
    pub no_operator: bool,

    #[arg(long = "cilium", default_value_t = false)]
    pub cilium: bool,
}

#[derive(Debug, Parser)]
pub struct BuildArgs {
    #[arg(index = 1)]
    pub ctf: PathBuf,

    #[arg(short = 'c', long = "challenge")]
    pub challenges: Vec<String>,

    #[arg(short, long, value_enum, default_value_t = Backend::Docker)]
    pub backend: Backend,

    #[arg(short, long, default_value_t = false)]
    pub publish: bool,
}

#[derive(Debug, Parser)]
pub struct UploadArgs {
    #[arg(index = 1)]
    pub ctf: PathBuf,

    #[arg(short = 'c', long = "challenge")]
    pub challenges: Vec<String>,

    #[arg(long)]
    pub access_key_id: Option<String>,

    #[arg(long)]
    pub secret_access_key: Option<String>,
}

#[derive(Debug, Clone, ValueEnum, Default)]
pub enum Backend {
    #[default]
    Docker,
    Buildah,
    Podman,
}

#[derive(Debug, Clone, ValueEnum, Default)]
pub enum ClusterBackend {
    #[default]
    Kind,
}

#[derive(Debug, Clone, ValueEnum, Default)]
pub enum KindProvider {
    #[default]
    Auto,
    Docker,
    Podman,
    Nerdctl,
}
