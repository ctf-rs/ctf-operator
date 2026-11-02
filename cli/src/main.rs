use anyhow::Result;
use clap::Parser;
use ctf_cli::{args::Args, ui::Ui};

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_existing| anyhow::anyhow!("a TLS crypto provider was already installed"))?;
    let args = Args::parse();
    let ui = Ui::new(args.plain);
    Box::pin(ui.execute(args.execute(ui.clone()))).await
}
