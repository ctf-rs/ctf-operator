use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context};
use tokio::{fs, process::Command};

use crate::{
    cli::{ClusterBackend, ClusterCreateArgs, KindProvider},
    read_challenge, read_ctf,
};
use ctf_cli::models::Ctf;

pub async fn create(args: ClusterCreateArgs) -> anyhow::Result<()> {
    let ClusterCreateArgs {
        ctf,
        namespaces,
        instance_namespace_prefix,
        backend,
        kind_provider,
        operator_namespace,
        operator_replicas,
        skip_cilium,
    } = args;

    let ctf_abs = std::fs::canonicalize(&ctf)
        .with_context(|| format!("failed to read ctf path {}", ctf.display()))?;
    let ctf_model = read_ctf(ctf_abs.clone()).await?;
    let cluster_name = ctf_model.name.clone();
    let expose_port_start = ctf_model.network.ports.start;
    let expose_port_end = ctf_model.network.ports.end;

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

            if kind_cluster_exists(&cluster_name, &provider).await? {
                delete_kind_cluster(&cluster_name, &provider).await?;
            }

            create_kind_cluster(&cluster_name, expose_port_start, expose_port_end, &provider)
                .await?;

            if !skip_cilium {
                install_cilium().await?;
            }

            let chart = pull_operator_chart().await?;
            install_operator_chart(
                &chart,
                &operator_namespace,
                operator_replicas,
                &namespaces,
                &instance_namespace_prefix,
                &cluster_name,
            )
            .await?;
        }
    }

    apply_ctf_bundle(&ctf_model, &ctf_abs, &cluster_name, &namespaces).await?;

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

async fn create_kind_cluster(
    cluster_name: &str,
    expose_port_start: u16,
    expose_port_end: u16,
    provider: &str,
) -> anyhow::Result<()> {
    let config_file = temp_file_path("kind-config", "yaml");
    let mut config = format!(
        "kind: Cluster\napiVersion: kind.x-k8s.io/v1alpha4\nkubeadmConfigPatches:\n- |\n  kind: ClusterConfiguration\n  apiServer:\n    extraArgs:\n      service-node-port-range: \"{expose_port_start}-{expose_port_end}\"\nnodes:\n- role: control-plane\n  extraPortMappings:\n"
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

async fn delete_kind_cluster(cluster_name: &str, provider: &str) -> anyhow::Result<()> {
    run_streamed_os_with_env(
        "kind",
        [
            OsStr::new("delete"),
            OsStr::new("cluster"),
            OsStr::new("--name"),
            OsStr::new(cluster_name),
        ],
        &[("KIND_EXPERIMENTAL_PROVIDER", provider)],
    )
    .await
}

async fn kind_cluster_exists(cluster_name: &str, provider: &str) -> anyhow::Result<bool> {
    let mut command = Command::new("kind");
    command.args(["get", "clusters"]);
    command.env("KIND_EXPERIMENTAL_PROVIDER", provider);
    let output = command
        .output()
        .await
        .context("failed to list kind clusters")?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().any(|line| line.trim() == cluster_name))
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

async fn pull_operator_chart() -> anyhow::Result<PathBuf> {
    let pull_dir = temp_dir_path("ctf-operator-chart");
    fs::create_dir_all(&pull_dir).await?;

    run_streamed_os(
        "helm",
        [
            OsStr::new("pull"),
            OsStr::new("oci://registry.ctf.rs/ctf/ctf-operator"),
            OsStr::new("--untar"),
            OsStr::new("--untardir"),
            pull_dir.as_os_str(),
        ],
    )
    .await?;

    Ok(pull_dir.join("ctf-operator"))
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
    chart: &Path,
    operator_namespace: &str,
    operator_replicas: u16,
    namespaces: &[String],
    instance_namespace_prefix: &str,
    cluster_name: &str,
) -> anyhow::Result<()> {
    if !chart.exists() {
        bail!("operator chart not found at {}", chart.display());
    }

    let image_pull_secret = ensure_registry_pull_secret(operator_namespace, cluster_name).await?;

    let values_path = temp_file_path("ctf-operator-values", "yaml");
    let mut values = format!(
        "replicaCount: {operator_replicas}\nimage:\n  repository: registry.ctf.rs/ctf/operator\n  tag: edge\n"
    );
    if let Some(image_pull_secret) = image_pull_secret {
        values.push_str("imagePullSecrets:\n");
        values.push_str(&format!("  - name: {image_pull_secret}\n"));
    }
    values.push_str("watchNamespaces:\n");
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

async fn ensure_registry_pull_secret(
    operator_namespace: &str,
    cluster_name: &str,
) -> anyhow::Result<Option<String>> {
    let registry_config = helm_registry_config_path()?;
    if !registry_config.exists() {
        return Ok(None);
    }

    ensure_namespace(operator_namespace, cluster_name).await?;

    let secret_name = "ctf-operator-registry-creds".to_owned();
    let from_file = format!(
        "--from-file=.dockerconfigjson={}",
        registry_config.display()
    );
    let output = run_capture_os(
        "kubectl",
        [
            OsStr::new("create"),
            OsStr::new("secret"),
            OsStr::new("generic"),
            OsStr::new(&secret_name),
            OsStr::new(&from_file),
            OsStr::new("--type=kubernetes.io/dockerconfigjson"),
            OsStr::new("--namespace"),
            OsStr::new(operator_namespace),
            OsStr::new("--context"),
            OsStr::new(&format!("kind-{cluster_name}")),
            OsStr::new("--dry-run=client"),
            OsStr::new("-o"),
            OsStr::new("yaml"),
        ],
    )
    .await?;

    let manifest_path = temp_file_path("ctf-operator-regcred", "yaml");
    fs::write(&manifest_path, output).await?;

    run_streamed_os(
        "kubectl",
        [
            OsStr::new("apply"),
            OsStr::new("-f"),
            manifest_path.as_os_str(),
            OsStr::new("--context"),
            OsStr::new(&format!("kind-{cluster_name}")),
        ],
    )
    .await?;

    Ok(Some(secret_name))
}

async fn ensure_namespace(operator_namespace: &str, cluster_name: &str) -> anyhow::Result<()> {
    let output = run_capture_os(
        "kubectl",
        [
            OsStr::new("create"),
            OsStr::new("namespace"),
            OsStr::new(operator_namespace),
            OsStr::new("--dry-run=client"),
            OsStr::new("-o"),
            OsStr::new("yaml"),
            OsStr::new("--context"),
            OsStr::new(&format!("kind-{cluster_name}")),
        ],
    )
    .await?;

    let manifest_path = temp_file_path("ctf-operator-namespace", "yaml");
    fs::write(&manifest_path, output).await?;

    run_streamed_os(
        "kubectl",
        [
            OsStr::new("apply"),
            OsStr::new("-f"),
            manifest_path.as_os_str(),
            OsStr::new("--context"),
            OsStr::new(&format!("kind-{cluster_name}")),
        ],
    )
    .await
}

fn helm_registry_config_path() -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("HELM_REGISTRY_CONFIG") {
        return Ok(PathBuf::from(path));
    }

    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/helm/registry/config.json"))
}

async fn run_capture_os<'a, I>(cmd: &str, args: I) -> anyhow::Result<Vec<u8>>
where
    I: IntoIterator<Item = &'a OsStr>,
{
    let mut command = Command::new(cmd);
    command.args(args);
    command.stdin(std::process::Stdio::null());
    command.stderr(std::process::Stdio::inherit());

    let output = command
        .output()
        .await
        .with_context(|| format!("failed to spawn command: {cmd}"))?;

    if !output.status.success() {
        bail!("command failed: {cmd} (exit {})", output.status);
    }

    Ok(output.stdout)
}

async fn apply_ctf_bundle(
    ctf: &Ctf,
    ctf_abs: &Path,
    cluster_name: &str,
    namespaces: &[String],
) -> anyhow::Result<()> {
    let ctf_dir = ctf_abs
        .parent()
        .context("ctf yaml must have parent directory")?
        .to_path_buf();

    let target_namespace = namespaces
        .first()
        .cloned()
        .context("at least one namespace is required")?;

    ensure_namespace(&target_namespace, cluster_name).await?;

    let mut core_challenges = Vec::new();
    let mut challenge_manifests = Vec::new();

    for challenge in &ctf.challenges {
        let challenge_path = ctf_dir.join(challenge);
        let core_challenge = read_challenge(challenge_path).await?;
        let challenge_manifest = ctf_cli::k8s::challenge_from_core(
            ctf,
            &core_challenge,
            Some(target_namespace.clone()),
        )?;
        core_challenges.push(core_challenge);
        challenge_manifests.push(challenge_manifest);
    }

    for challenge_manifest in challenge_manifests {
        let challenge_manifest_path = temp_file_path("challenge-manifest", "yaml");
        let challenge_manifest_yaml = serde_saphyr::to_string(&challenge_manifest)?;
        fs::write(&challenge_manifest_path, challenge_manifest_yaml).await?;
        run_streamed_os(
            "kubectl",
            [
                OsStr::new("apply"),
                OsStr::new("-f"),
                challenge_manifest_path.as_os_str(),
                OsStr::new("--context"),
                OsStr::new(&format!("kind-{cluster_name}")),
            ],
        )
        .await?;
    }

    let ctf_manifest = ctf_cli::k8s::ctf_from_core(ctf, &core_challenges, Some(target_namespace));
    let ctf_manifest_path = temp_file_path("ctf-manifest", "yaml");
    let ctf_manifest_yaml = serde_saphyr::to_string(&ctf_manifest)?;
    fs::write(&ctf_manifest_path, ctf_manifest_yaml).await?;

    run_streamed_os(
        "kubectl",
        [
            OsStr::new("apply"),
            OsStr::new("-f"),
            ctf_manifest_path.as_os_str(),
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

fn temp_dir_path(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}
