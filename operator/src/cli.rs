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
pub struct CrdArgs {}

#[derive(Debug, clap::ValueEnum, Clone, Copy)]
pub enum CrdFormat {
    Yaml,
    Json,
}

#[derive(Debug, Parser)]
pub struct OperatorArgs {}
