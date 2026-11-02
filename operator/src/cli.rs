use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[clap(about = "CTF Operator")]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Crd(CrdArgs),
    Operator(OperatorArgs),
}

#[derive(Debug, Parser)]
pub struct CrdArgs {
    #[arg(long, default_value = "crd")]
    pub output: PathBuf,
}

#[derive(Debug, Parser)]
pub struct OperatorArgs {
    #[arg(
        short = 'n',
        long = "namespace",
        value_delimiter = ',',
        default_value = "ctf"
    )]
    pub namespaces: Vec<String>,

    #[arg(long = "instance-namespace-prefix", default_value = "ctf-inst-")]
    pub instance_namespace_prefix: String,

    #[arg(long = "config", env = "CTF_OPERATOR_CONFIG_PATH")]
    pub config: Option<PathBuf>,
}
