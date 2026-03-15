use anyhow::{bail, Context};
use ctf_cli::models::ContainerImage;
use std::{collections::HashSet, path::Path};
use tokio::process::Command;

use crate::{
    cli::{Backend, BuildArgs},
    read_challenge, read_ctf,
};

#[expect(
    clippy::too_many_lines,
    reason = "build orchestration logic is inherently linear"
)]
pub async fn build(args: BuildArgs) -> anyhow::Result<()> {
    let BuildArgs {
        ctf,
        challenges,
        backend,
        publish,
    } = args;

    let ctf_dir = ctf
        .parent()
        .context("ctf yaml must have parent directory")?
        .to_path_buf();

    let ctf = read_ctf(ctf).await?;

    if ctf.challenges.is_empty() {
        bail!("ctf has no challenges configured");
    }

    let mut challenge_items = Vec::new();
    let mut seen_challenge_names = HashSet::<String>::new();
    for challenge_rel_path in &ctf.challenges {
        let challenge_path = ctf_dir.join(challenge_rel_path);
        let challenge = read_challenge(challenge_path.clone()).await?;

        if !seen_challenge_names.insert(challenge.name.clone()) {
            bail!(
                "duplicate challenge name in ctf challenge files: {}",
                challenge.name
            );
        }

        challenge_items.push((challenge_path, challenge));
    }

    let requested_names = challenges.into_iter().collect::<HashSet<_>>();
    let selected = if requested_names.is_empty() {
        challenge_items
    } else {
        let selected = challenge_items
            .into_iter()
            .filter(|(_, challenge)| requested_names.contains(&challenge.name))
            .collect::<Vec<_>>();

        let available = selected
            .iter()
            .map(|(_, challenge)| challenge.name.clone())
            .collect::<HashSet<_>>();
        let missing = requested_names
            .difference(&available)
            .cloned()
            .collect::<Vec<_>>();

        if !missing.is_empty() {
            bail!(
                "requested challenge name(s) not found in ctf challenge YAML names: {}",
                missing.join(", ")
            );
        }

        selected
    };

    let publish_registry = if publish {
        Some(
            ctf.registry
                .as_ref()
                .context("--publish requires ctf.registry.url")?
                .url
                .trim_end_matches('/')
                .to_owned(),
        )
    } else {
        None
    };

    for (challenge_path, challenge) in selected {
        let challenge_dir = challenge_path
            .parent()
            .context("challenge yaml must have parent directory")?
            .to_path_buf();

        for container in &challenge.containers {
            if let ContainerImage::Dockerfile { path } = &container.image {
                let dockerfile_path = challenge_dir.join(path);

                if !dockerfile_path.exists() {
                    bail!(
                        "Container: {} Dockerfile not found at {}",
                        container.name,
                        dockerfile_path.display()
                    );
                }

                let context_dir = dockerfile_path
                    .parent()
                    .context("Dockerfile must have parent directory")?;

                let local_tag = format!("{}/{}:latest", challenge.name, container.name);
                let publish_tag = publish_registry.as_ref().map(|registry| {
                    format!("{registry}/{}/{}:latest", challenge.name, container.name)
                });

                println!("Building container {} with {:?}", container.name, backend);

                match backend {
                    Backend::Docker => {
                        docker_build(
                            &dockerfile_path,
                            context_dir,
                            &local_tag,
                            publish_tag.as_deref(),
                        )
                        .await?;
                    }
                    Backend::Buildah => {
                        buildah_build(
                            &dockerfile_path,
                            context_dir,
                            &local_tag,
                            publish_tag.as_deref(),
                        )
                        .await?;
                    }
                    Backend::Podman => {
                        podman_build(
                            &dockerfile_path,
                            context_dir,
                            &local_tag,
                            publish_tag.as_deref(),
                        )
                        .await?;
                    }
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
        .with_context(|| format!("failed to spawn command: {cmd:?}"))?;

    if !status.success() {
        bail!("command failed with exit status: {status}");
    }

    Ok(())
}

/// Build an image using Docker.
/// Equivalent to: `docker build -f <Dockerfile> -t <tag> <context>`
pub async fn docker_build(
    dockerfile_path: &Path,
    context_dir: &Path,
    local_tag: &str,
    publish_tag: Option<&str>,
) -> anyhow::Result<()> {
    let mut cmd = Command::new("docker");
    cmd.arg("build")
        .arg("-f")
        .arg(dockerfile_path)
        .arg("-t")
        .arg(local_tag)
        .arg(context_dir);

    run_streamed(cmd).await?;

    if let Some(publish_tag) = publish_tag {
        if publish_tag != local_tag {
            let mut cmd = Command::new("docker");
            cmd.arg("tag").arg(local_tag).arg(publish_tag);
            run_streamed(cmd).await?;
        }

        let mut cmd = Command::new("docker");
        cmd.arg("push").arg(publish_tag);

        run_streamed(cmd).await?;
    }

    Ok(())
}

/// Build an image using Buildah.
/// Equivalent to: `buildah bud -f <Dockerfile> -t <tag> <context>`
pub async fn buildah_build(
    dockerfile_path: &Path,
    context_dir: &Path,
    local_tag: &str,
    publish_tag: Option<&str>,
) -> anyhow::Result<()> {
    let mut cmd = Command::new("buildah");
    cmd.arg("bud")
        .arg("-f")
        .arg(dockerfile_path)
        .arg("-t")
        .arg(local_tag)
        .arg(context_dir);

    run_streamed(cmd).await?;

    if let Some(publish_tag) = publish_tag {
        if publish_tag != local_tag {
            let mut cmd = Command::new("buildah");
            cmd.arg("tag").arg(local_tag).arg(publish_tag);
            run_streamed(cmd).await?;
        }

        let mut cmd = Command::new("buildah");
        cmd.arg("push")
            .arg(publish_tag)
            .arg(format!("docker://{publish_tag}"));

        run_streamed(cmd).await?;
    }

    Ok(())
}

pub async fn podman_build(
    dockerfile_path: &Path,
    context_dir: &Path,
    local_tag: &str,
    publish_tag: Option<&str>,
) -> anyhow::Result<()> {
    let mut cmd = Command::new("podman");
    cmd.arg("build")
        .arg("-f")
        .arg(dockerfile_path)
        .arg("-t")
        .arg(local_tag)
        .arg(context_dir);

    run_streamed(cmd).await?;

    if let Some(publish_tag) = publish_tag {
        if publish_tag != local_tag {
            let mut cmd = Command::new("podman");
            cmd.arg("tag").arg(local_tag).arg(publish_tag);
            run_streamed(cmd).await?;
        }

        let mut cmd = Command::new("podman");
        cmd.arg("push").arg(publish_tag);
        run_streamed(cmd).await?;
    }

    Ok(())
}
