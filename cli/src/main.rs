use std::path::PathBuf;

use anyhow::bail;
use clap::Parser;
use ctf_core::{Challenge, Ctf};

pub mod build;
pub mod cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();
    match args.cmd {
        cli::Command::Build(args) => build::build(args).await?,
    }

    Ok(())
}

pub async fn read_challenge(path: PathBuf) -> anyhow::Result<Challenge> {
    if !path.exists() {
        bail!("Challenge file doesn't exist");
    }

    let data = tokio::fs::read_to_string(&path).await?;
    let challenge: Challenge = serde_saphyr::from_str(&data)?;
    challenge.validate_strict_at(&path)?;
    Ok(challenge)
}

pub async fn read_ctf(path: PathBuf) -> anyhow::Result<Ctf> {
    if !path.exists() {
        bail!("CTF file doesn't exist");
    }

    let data = tokio::fs::read_to_string(&path).await?;
    let ctf: Ctf = serde_saphyr::from_str(&data)?;
    ctf.validate_strict_at(&path)?;
    Ok(ctf)
}
