use clap::Parser;
use ctf_operator::Args;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    Ok(())
}
