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
