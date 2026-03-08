use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context};
use tokio::{fs, process::Command};

use crate::{
    cli::{ClusterBackend, ClusterCreateArgs, KindProvider},
    read_ctf,
};

pub async fn create(args: ClusterCreateArgs) -> anyhow::Result<()> {
    let ClusterCreateArgs {
        cluster_name,
        namespaces,
        instance_namespace_prefix,
        ctf,
        backend,
        kind_provider,
        operator_namespace,
        operator_replicas,
        expose_port_start,
        expose_port_end,
        skip_cilium,
    } = args;

    if expose_port_start > expose_port_end {
        bail!("--expose-port-start must be <= --expose-port-end");
    }

    let namespaces = namespaces
        .into_iter()
        .filter(|ns| !ns.trim().is_empty())
        .collect::<Vec<_>>();
    if namespaces.is_empty() {
        bail!("at least one namespace is required");
    }

    if instance_namespace_prefix.trim().is_empty() {
        bail!("--instance-namespace-prefix must be non-empty");
    }

    match backend {
        ClusterBackend::Kind => {
            let provider = resolve_kind_provider(kind_provider).await?;
            ensure_command("kind").await?;
            ensure_command("kubectl").await?;
            ensure_command("helm").await?;
            ensure_command(kind_provider_command(&provider)).await?;
            if !skip_cilium {
                ensure_command("cilium").await?;
            }

            create_kind_cluster(&cluster_name, expose_port_start, expose_port_end, &provider)
                .await?;

            if !skip_cilium {
                install_cilium().await?;
            }

            build_operator_image().await?;
            kind_load_operator_image(&cluster_name, &provider).await?;
            install_operator_chart(
                &operator_namespace,
                operator_replicas,
                &namespaces,
                &instance_namespace_prefix,
                &cluster_name,
            )
            .await?;
        }
    }

    apply_ctf_bundle(ctf, &cluster_name).await?;

    Ok(())
}

async fn ensure_command(cmd: &str) -> anyhow::Result<()> {
    let mut command = Command::new(cmd);
    command.arg("--help");

    let status = command
        .status()
        .await
        .with_context(|| format!("required command not found: {cmd}"))?;

    if !status.success() {
        bail!("required command failed: {cmd} --help");
    }

    Ok(())
}

async fn run_streamed(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    run_streamed_os(cmd, args.iter().map(OsStr::new)).await
}

async fn run_streamed_os_with_env<'a, I>(
    cmd: &str,
    args: I,
    envs: &[(&str, &str)],
) -> anyhow::Result<()>
where
    I: IntoIterator<Item = &'a OsStr>,
{
    let mut command = Command::new(cmd);
    command.args(args);
    command.envs(envs.iter().copied());
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::inherit());
    command.stderr(std::process::Stdio::inherit());

    let status = command
        .status()
        .await
        .with_context(|| format!("failed to spawn command: {cmd}"))?;

    if !status.success() {
        bail!("command failed: {cmd} (exit {status})");
    }

    Ok(())
}

async fn run_streamed_os<'a, I>(cmd: &str, args: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = &'a OsStr>,
{
    let mut command = Command::new(cmd);
    command.args(args);
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::inherit());
    command.stderr(std::process::Stdio::inherit());

    let status = command
        .status()
        .await
        .with_context(|| format!("failed to spawn command: {cmd}"))?;

    if !status.success() {
        bail!("command failed: {cmd} (exit {status})");
    }

    Ok(())
}

fn workspace_root() -> anyhow::Result<PathBuf> {
    let cli_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    cli_dir
        .parent()
        .map(Path::to_path_buf)
        .context("failed to detect workspace root")
}

async fn create_kind_cluster(
    cluster_name: &str,
    expose_port_start: u16,
    expose_port_end: u16,
    provider: &str,
) -> anyhow::Result<()> {
    let config_file = temp_file_path("kind-config", "yaml");
    let mut config = String::from(
        "kind: Cluster\napiVersion: kind.x-k8s.io/v1alpha4\nnodes:\n- role: control-plane\n  extraPortMappings:\n",
    );

    for port in expose_port_start..=expose_port_end {
        config.push_str(&format!(
            "  - containerPort: {port}\n    hostPort: {port}\n    listenAddress: \"0.0.0.0\"\n    protocol: TCP\n"
        ));
    }

    fs::write(&config_file, config).await?;

    run_streamed_os_with_env(
        "kind",
        [
            OsStr::new("create"),
            OsStr::new("cluster"),
            OsStr::new("--name"),
            OsStr::new(cluster_name),
            OsStr::new("--config"),
            config_file.as_os_str(),
        ],
        &[("KIND_EXPERIMENTAL_PROVIDER", provider)],
    )
    .await?;

    run_streamed("kubectl", &["cluster-info"]).await?;
    Ok(())
}

async fn install_cilium() -> anyhow::Result<()> {
    run_streamed(
        "cilium",
        &[
            "install",
            "--wait",
            "--set",
            "kubeProxyReplacement=true",
            "--set",
            "ipam.mode=kubernetes",
        ],
    )
    .await
}

async fn build_operator_image() -> anyhow::Result<()> {
    let root = workspace_root()?;
    run_streamed_os(
        "docker",
        [
            OsStr::new("build"),
            OsStr::new("-t"),
            OsStr::new("ctf-operator:local"),
            root.as_os_str(),
        ],
    )
    .await
}

async fn kind_load_operator_image(cluster_name: &str, provider: &str) -> anyhow::Result<()> {
    run_streamed_os_with_env(
        "kind",
        [
            OsStr::new("load"),
            OsStr::new("docker-image"),
            OsStr::new("ctf-operator:local"),
            OsStr::new("--name"),
            OsStr::new(cluster_name),
        ],
        &[("KIND_EXPERIMENTAL_PROVIDER", provider)],
    )
    .await
}

fn kind_provider_command(provider: &str) -> &'static str {
    match provider {
        "docker" => "docker",
        "podman" => "podman",
        "nerdctl" => "nerdctl",
        _ => "docker",
    }
}

async fn resolve_kind_provider(provider: KindProvider) -> anyhow::Result<String> {
    match provider {
        KindProvider::Docker => return Ok("docker".to_owned()),
        KindProvider::Podman => return Ok("podman".to_owned()),
        KindProvider::Nerdctl => return Ok("nerdctl".to_owned()),
        KindProvider::Auto => {}
    }

    if command_succeeds("docker", &["info"]).await {
        return Ok("docker".to_owned());
    }
    if command_succeeds("podman", &["info"]).await {
        return Ok("podman".to_owned());
    }
    if command_succeeds("nerdctl", &["info"]).await {
        return Ok("nerdctl".to_owned());
    }

    bail!("failed to detect kind provider; use --kind-provider docker|podman|nerdctl")
}

async fn command_succeeds(cmd: &str, args: &[&str]) -> bool {
    let mut command = Command::new(cmd);
    command.args(args);
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());

    match command.status().await {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

async fn install_operator_chart(
    operator_namespace: &str,
    operator_replicas: u16,
    namespaces: &[String],
    instance_namespace_prefix: &str,
    cluster_name: &str,
) -> anyhow::Result<()> {
    let root = workspace_root()?;
    let chart = root.join("helm/ctf-operator");
    if !chart.exists() {
        bail!("operator chart not found at {}", chart.display());
    }

    let values_path = temp_file_path("ctf-operator-values", "yaml");
    let mut values = format!(
        "replicaCount: {operator_replicas}\nimage:\n  repository: ctf-operator\n  tag: local\n  pullPolicy: IfNotPresent\nwatchNamespaces:\n"
    );
    for namespace in namespaces {
        values.push_str(&format!("  - {namespace}\n"));
    }
    values.push_str(&format!(
        "instanceNamespacePrefix: {}\n",
        instance_namespace_prefix
    ));

    fs::write(&values_path, values).await?;

    run_streamed_os(
        "helm",
        [
            OsStr::new("upgrade"),
            OsStr::new("--install"),
            OsStr::new("ctf-operator"),
            chart.as_os_str(),
            OsStr::new("--namespace"),
            OsStr::new(operator_namespace),
            OsStr::new("--create-namespace"),
            OsStr::new("--wait"),
            OsStr::new("--wait-for-jobs"),
            OsStr::new("--kube-context"),
            OsStr::new(&format!("kind-{cluster_name}")),
            OsStr::new("-f"),
            values_path.as_os_str(),
        ],
    )
    .await
}

async fn apply_ctf_bundle(ctf_path: PathBuf, cluster_name: &str) -> anyhow::Result<()> {
    let ctf_abs = std::fs::canonicalize(&ctf_path)
        .with_context(|| format!("failed to read ctf path {}", ctf_path.display()))?;

    let ctf = read_ctf(ctf_abs.clone()).await?;
    let ctf_dir = ctf_abs
        .parent()
        .context("ctf yaml must have parent directory")?
        .to_path_buf();

    for challenge in ctf.challenges {
        let challenge_path = ctf_dir.join(challenge);
        run_streamed_os(
            "kubectl",
            [
                OsStr::new("apply"),
                OsStr::new("-f"),
                challenge_path.as_os_str(),
                OsStr::new("--context"),
                OsStr::new(&format!("kind-{cluster_name}")),
            ],
        )
        .await?;
    }

    run_streamed_os(
        "kubectl",
        [
            OsStr::new("apply"),
            OsStr::new("-f"),
            ctf_abs.as_os_str(),
            OsStr::new("--context"),
            OsStr::new(&format!("kind-{cluster_name}")),
        ],
    )
    .await?;

    Ok(())
}

fn temp_file_path(prefix: &str, ext: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{nanos}.{ext}"))
}
