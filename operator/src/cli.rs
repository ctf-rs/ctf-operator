use clap::{Parser, Subcommand};

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
    #[arg(short = 'n', long = "namespace", default_value = "ctf")]
    pub namespace: String,
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
}
