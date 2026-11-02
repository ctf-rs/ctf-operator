use std::path::PathBuf;

use anyhow::Result;
use clap::{Args as ClapArgs, Parser, Subcommand};

use crate::{build, cluster, manifest::Project, test, ui::Ui};

#[derive(Debug, Parser)]
#[command(name = "ctf", about = "Build and test CTF challenges locally")]
pub struct Args {
    #[arg(long, global = true)]
    pub plain: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Validate(ProjectArgs),
    Build(ProjectArgs),
    Test(TestArgs),
    Cluster {
        #[command(subcommand)]
        command: ClusterCommand,
    },
}

#[derive(Debug, Clone, ClapArgs)]
pub struct ProjectArgs {
    pub manifest: PathBuf,
    #[arg(short = 'c', long = "challenge")]
    pub challenges: Vec<String>,
    #[arg(long, default_value = "dist")]
    pub output: PathBuf,
}

#[derive(Debug, ClapArgs)]
pub struct TestArgs {
    #[command(flatten)]
    pub project: ProjectArgs,
    #[arg(long)]
    pub keep: bool,
}

#[derive(Debug, Subcommand)]
pub enum ClusterCommand {
    Create {
        manifest: PathBuf,
        /// Locally built operator image to import into kind.
        #[arg(long, default_value = "ctf-operator:local")]
        operator_image: String,
    },
    Status {
        manifest: PathBuf,
    },
    Delete {
        manifest: PathBuf,
    },
}

impl Args {
    pub async fn execute(self, ui: Ui) -> Result<()> {
        match self.command {
            Command::Validate(args) => {
                let project = Project::load(&args.manifest, &args.challenges)?;
                ui.message(format!("Validated {} challenges", project.challenges.len()));
                Ok(())
            }
            Command::Build(args) => {
                let project = Project::load(&args.manifest, &args.challenges)?;
                build::project(&project, &args.output, &ui).await?;
                Ok(())
            }
            Command::Test(args) => test::project(args, &ui).await,
            Command::Cluster { command } => match command {
                ClusterCommand::Create {
                    manifest,
                    operator_image,
                } => {
                    let project = Project::load(&manifest, &[])?;
                    Box::pin(cluster::create(&project, &operator_image, &ui)).await
                }
                ClusterCommand::Status { manifest } => {
                    cluster::status(&Project::load(&manifest, &[])?, &ui).await
                }
                ClusterCommand::Delete { manifest } => {
                    cluster::delete(&Project::load(&manifest, &[])?, &ui).await
                }
            },
        }
    }
}
