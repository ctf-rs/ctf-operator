use anyhow::{bail, Context};
use ctf_core::ContainerImage;
use std::path::Path;
use tokio::process::Command;

use crate::{
    cli::{Backend, BuildArgs},
    read_challenge,
};

pub async fn build(args: BuildArgs) -> anyhow::Result<()> {
    let BuildArgs {
        challenge,
        backend,
        publish,
    } = args;

    let challenge_dir = challenge
        .parent()
        .context("challenge yaml must have parent directory")?
        .to_path_buf();

    let challenge = read_challenge(challenge).await?;

    for container in &challenge.containers {
        if let ContainerImage::Dockerfile { path } = &container.image {
            let dockerfile_path = challenge_dir.join(path);

            if !dockerfile_path.exists() {
                bail!(
                    "Container: {} Dockerfile not found at {:?}",
                    container.name,
                    dockerfile_path
                );
            }

            let context_dir = dockerfile_path
                .parent()
                .context("Dockerfile must have parent directory")?;

            let tag = container.name.as_str();

            println!("Building container {} with {:?}", container.name, backend);

            match backend {
                Backend::Docker => {
                    docker_build(&dockerfile_path, context_dir, tag, publish).await?;
                }
                Backend::Buildah => {
                    buildah_build(&dockerfile_path, context_dir, tag, publish).await?;
                }
            }
        }
    }

    Ok(())
}

/// Run a command and stream stdout/stderr to the current terminal.
/// Returns error if exit status != 0.
async fn run_streamed(mut cmd: Command) -> anyhow::Result<()> {
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    let status = cmd
        .status()
        .await
        .with_context(|| format!("failed to spawn command: {:?}", cmd))?;

    if !status.success() {
        bail!("command failed with exit status: {}", status);
    }

    Ok(())
}

/// Build an image using Docker.
/// Equivalent to: `docker build -f <Dockerfile> -t <tag> <context>`
pub async fn docker_build(
    dockerfile_path: &Path,
    context_dir: &Path,
    tag: &str,
    publish: bool,
) -> anyhow::Result<()> {
    let mut cmd = Command::new("docker");
    cmd.arg("build")
        .arg("-f")
        .arg(dockerfile_path)
        .arg("-t")
        .arg(tag)
        .arg(context_dir);

    run_streamed(cmd).await?;

    if publish {
        let mut cmd = Command::new("docker");
        cmd.arg("push").arg(tag);

        run_streamed(cmd).await?;
    }

    Ok(())
}

/// Build an image using Buildah.
/// Equivalent to: `buildah bud -f <Dockerfile> -t <tag> <context>`
pub async fn buildah_build(
    dockerfile_path: &Path,
    context_dir: &Path,
    tag: &str,
    publish: bool,
) -> anyhow::Result<()> {
    let mut cmd = Command::new("buildah");
    cmd.arg("bud")
        .arg("-f")
        .arg(dockerfile_path)
        .arg("-t")
        .arg(tag)
        .arg(context_dir);

    run_streamed(cmd).await?;

    if publish {
        let mut cmd = Command::new("buildah");
        cmd.arg("push").arg(tag).arg(format!("docker://{}", tag));

        run_streamed(cmd).await?;
    }

    Ok(())
}
