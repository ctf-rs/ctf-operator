use std::{
    env,
    fmt::Debug,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, ensure, Context, Result};
use ctf_core::{
    k8s::{Challenge, Ctf as CtfResource, CtfSpec, Flag, Instance, PortClaim, ReservationSet},
    models::CtfPorts,
};
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{ConfigMap, Namespace, ServiceAccount},
        rbac::v1::{ClusterRole, ClusterRoleBinding},
    },
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
};
use kube::{
    api::{ObjectMeta, Patch, PatchParams},
    config::{KubeConfigOptions, Kubeconfig},
    runtime::wait::{await_condition, conditions},
    Api, Client, Config, CustomResourceExt, Resource, ResourceExt,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use tokio::process::Command;

use crate::{
    manifest::Project,
    process::{self, Format},
    ui::Ui,
};

pub const NAMESPACE: &str = "ctf-local";
/// kind names the node `<cluster>-control-plane`, which must remain a 63-character DNS label.
const MAX_CLUSTER_NAME: usize = 63 - "-control-plane".len();

fn network_name(cluster: &str) -> String {
    format!("ctf-{cluster}")
}

fn kind(name: &str) -> Command {
    let mut command = Command::new("kind");
    command.env("KIND_EXPERIMENTAL_PROVIDER", "docker");
    command.env("KIND_EXPERIMENTAL_DOCKER_NETWORK", network_name(name));
    command
}

async fn ensure_network(name: &str, directory: &Path, ui: &Ui) -> Result<()> {
    let network = network_name(name);
    let output = Command::new("docker")
        .args([
            "network",
            "ls",
            "--filter",
            &format!("name=^{network}$"),
            "--format",
            "{{.Name}}",
        ])
        .output()
        .await
        .context("could not list Docker networks")?;
    ensure!(
        output.status.success(),
        "Docker network listing failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    if String::from_utf8(output.stdout)?
        .lines()
        .any(|candidate| candidate == network)
    {
        let output = Command::new("docker")
            .args([
                "network",
                "inspect",
                "--format",
                "{{index .Labels \"ctf.rs/cluster\"}}",
                &network,
            ])
            .output()
            .await?;
        ensure!(
            output.status.success(),
            "could not inspect dedicated network: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        ensure!(
            String::from_utf8(output.stdout)?.trim() == name,
            "refusing to adopt unowned Docker network {network}"
        );
    } else {
        let mut command = Command::new("docker");
        command.args([
            "network",
            "create",
            "--driver",
            "bridge",
            "--label",
            &format!("ctf.rs/cluster={name}"),
            &network,
        ]);
        run(
            command,
            "Create dedicated cluster network",
            &directory.join("network-create"),
            ui,
        )
        .await?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LocalCluster {
    pub name: String,
    pub ports: CtfPorts,
}

impl LocalCluster {
    pub fn directory(name: &str) -> Result<PathBuf> {
        ensure!(
            name != "kind"
                && !name.is_empty()
                && name.len() <= MAX_CLUSTER_NAME
                && name
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                && !name.starts_with('-')
                && !name.ends_with('-'),
            "event name {name} cannot name a kind cluster: it must be a DNS label of at most {MAX_CLUSTER_NAME} characters"
        );
        let root = env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
            .context("neither XDG_STATE_HOME nor HOME is set")?;
        ensure!(root.is_absolute(), "state directory must be absolute");
        Ok(root.join("ctf/clusters").join(name))
    }

    /// Loads the cluster owned for the event `name`.
    pub fn load(name: &str) -> Result<Self> {
        let path = Self::directory(name)?.join("cluster.json");
        let state: Self = serde_json::from_slice(&fs::read(&path).with_context(|| {
            format!("event {name} has no local cluster; run `ctf cluster create <ctf.yaml>` first")
        })?)?;
        ensure!(
            state.name == name,
            "cluster ownership record does not match {name}"
        );
        Ok(state)
    }

    #[must_use]
    pub fn network(&self) -> String {
        network_name(&self.name)
    }

    /// IP address of the control-plane node on the dedicated cluster network.
    pub async fn node_address(&self) -> Result<String> {
        let network = self.network();
        let output = Command::new("docker")
            .args([
                "inspect",
                "--format",
                &format!("{{{{(index .NetworkSettings.Networks \"{network}\").IPAddress}}}}"),
                &format!("{}-control-plane", self.name),
            ])
            .output()
            .await
            .context("could not inspect the kind node")?;
        ensure!(
            output.status.success(),
            "could not resolve node address: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let address: std::net::IpAddr = String::from_utf8(output.stdout)?
            .trim()
            .parse()
            .context("kind node has no address on its dedicated network")?;
        Ok(address.to_string())
    }

    pub async fn client(&self) -> Result<Client> {
        let path = Self::directory(&self.name)?.join("kubeconfig");
        let kubeconfig = Kubeconfig::read_from(&path)
            .with_context(|| format!("cannot read cluster kubeconfig {}", path.display()))?;
        let config =
            Config::from_custom_kubeconfig(kubeconfig, &KubeConfigOptions::default()).await?;
        Client::try_from(config).context("could not connect to local cluster")
    }

    pub fn supports(&self, ports: &CtfPorts) -> Result<()> {
        ensure!(
            ports.start >= self.ports.start && ports.end <= self.ports.end,
            "event port range {}-{} is outside cluster {} range {}-{}; explicitly recreate the cluster to change mappings",
            ports.start, ports.end, self.name, self.ports.start, self.ports.end
        );
        Ok(())
    }
}

/// Creates or updates the event's cluster, named after the event, with its operator and
/// its single `Ctf`.
pub async fn create(project: &Project, operator_image: &str, ui: &Ui) -> Result<()> {
    let ctf = &project.ctf;
    let name = ctf.name.as_str();
    let directory = LocalCluster::directory(name)?;
    let existing = exists(name).await?;
    let state = if existing {
        let state = LocalCluster::load(name)
            .context("refusing to adopt or delete an existing unowned kind cluster")?;
        state.supports(&ctf.network.ports)?;
        state
    } else {
        fs::create_dir_all(&directory)?;
        let state = LocalCluster {
            name: name.to_owned(),
            ports: ctf.network.ports.clone(),
        };
        fs::write(
            directory.join("cluster.json"),
            serde_json::to_vec_pretty(&state)?,
        )?;
        let config = configuration(&state)?;
        fs::write(
            directory.join("kind.yaml"),
            serde_saphyr::to_string(&config)?,
        )?;
        ensure_network(name, &directory, ui).await?;
        let mut command = kind(name);
        command.args(["create", "cluster", "--name", name, "--config"]);
        command.arg(directory.join("kind.yaml"));
        command
            .arg("--kubeconfig")
            .arg(directory.join("kubeconfig"));
        run(
            command,
            "Create kind cluster",
            &directory.join("create"),
            ui,
        )
        .await?;
        state
    };
    let kubeconfig = directory.join("kubeconfig");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&kubeconfig, fs::Permissions::from_mode(0o600))?;
    }
    let client = state.client().await?;
    Box::pin(install_crds(client.clone(), ui)).await?;
    let mut inspect = Command::new("docker");
    inspect.args(["image", "inspect", "--format", "{{.Id}}", operator_image]);
    let output = inspect
        .output()
        .await
        .context("could not inspect operator image")?;
    if !output.status.success() {
        bail!(
            "operator image {operator_image} is not available locally; build the operator Dockerfile first, or select a local image with --operator-image: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let image_id = String::from_utf8(output.stdout)?.trim().to_owned();
    ensure!(
        image_id.starts_with("sha256:") && image_id.len() == 71,
        "operator image has an invalid image ID"
    );
    let image = format!("ctf-local/operator:{}", &image_id[7..19]);
    let mut tag = Command::new("docker");
    tag.args(["tag", operator_image, &image]);
    run(
        tag,
        "Resolve operator image",
        &directory.join("operator-tag"),
        ui,
    )
    .await?;
    load_images(
        name,
        std::slice::from_ref(&image),
        &directory.join("operator-load"),
        ui,
    )
    .await?;
    Box::pin(install_operator(client.clone(), &image, ui)).await?;
    apply(&Api::namespaced(client, NAMESPACE), &event_ctf(project)).await?;
    ui.message(format!(
        "Cluster {name} ready; kubeconfig: {}",
        kubeconfig.display()
    ));
    Ok(())
}

fn configuration(state: &LocalCluster) -> Result<serde_json::Value> {
    let patch = serde_saphyr::to_string(&json!({
        "apiVersion": "kubeadm.k8s.io/v1beta4",
        "kind": "ClusterConfiguration",
        "apiServer": { "extraArgs": [{
            "name": "service-node-port-range",
            "value": format!("{}-{}", state.ports.start, state.ports.end)
        }]}
    }))?;
    let mappings: Vec<_> = (state.ports.start..=state.ports.end)
        .map(|port| {
            json!({
                "containerPort": port, "hostPort": port,
                "listenAddress": "127.0.0.1", "protocol": "TCP"
            })
        })
        .collect();
    Ok(json!({
        "kind": "Cluster",
        "apiVersion": "kind.x-k8s.io/v1alpha4",
        "kubeadmConfigPatches": [patch],
        "nodes": [{ "role": "control-plane", "extraPortMappings": mappings }]
    }))
}

async fn exists(name: &str) -> Result<bool> {
    let output = kind(name)
        .args(["get", "clusters"])
        .output()
        .await
        .context("could not list kind clusters")?;
    ensure!(
        output.status.success(),
        "kind get clusters failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .any(|line| line == name))
}

/// The event's single `Ctf`, referencing every challenge of the event.
#[must_use]
pub fn event_ctf(project: &Project) -> CtfResource {
    let ctf = &project.ctf;
    CtfResource {
        metadata: ObjectMeta {
            name: Some(ctf.name.clone()),
            namespace: Some(NAMESPACE.to_owned()),
            ..ObjectMeta::default()
        },
        spec: CtfSpec {
            full_name: ctf.full_name.clone(),
            description: ctf.description.clone(),
            public: false,
            instance_duration_seconds: ctf.instance_duration_seconds,
            challenges: project.names.clone(),
            flag: ctf.flag.clone(),
            network: ctf.network.clone(),
            scoreboard: ctf.scoreboard.clone(),
            schedule: ctf.schedule.clone(),
        },
        status: None,
    }
}

pub async fn status(project: &Project, ui: &Ui) -> Result<()> {
    let name = project.ctf.name.as_str();
    let state = LocalCluster::load(name)?;
    ensure!(exists(name).await?, "owned cluster {name} does not exist");
    let client = state.client().await?;
    let deployment = Api::<Deployment>::namespaced(client, NAMESPACE)
        .get("ctf-operator")
        .await?;
    let ready = deployment
        .status
        .as_ref()
        .and_then(|status| status.available_replicas)
        .unwrap_or(0);
    ui.message(format!(
        "Cluster {name}: operator {ready}/1 available, TCP {}-{}",
        state.ports.start, state.ports.end
    ));
    ensure!(ready > 0, "operator is not available");
    Ok(())
}

pub async fn delete(project: &Project, ui: &Ui) -> Result<()> {
    let name = project.ctf.name.as_str();
    let _state = LocalCluster::load(name).context("refusing to delete an unowned cluster")?;
    let directory = LocalCluster::directory(name)?;
    let mut command = kind(name);
    command.args(["delete", "cluster", "--name", name]);
    command
        .arg("--kubeconfig")
        .arg(directory.join("kubeconfig"));
    run(
        command,
        "Delete owned kind cluster",
        &directory.join("delete"),
        ui,
    )
    .await?;
    fs::remove_file(directory.join("cluster.json"))?;
    if directory.join("kubeconfig").exists() {
        fs::remove_file(directory.join("kubeconfig"))?;
    }
    ui.message(format!(
        "Deleted cluster {name}; diagnostic logs retained at {}",
        directory.display()
    ));
    Ok(())
}

pub async fn load_images(name: &str, images: &[String], log: &Path, ui: &Ui) -> Result<()> {
    if images.is_empty() {
        return Ok(());
    }
    let _state = LocalCluster::load(name)?;
    let mut command = kind(name);
    command
        .args(["load", "docker-image", "--name", name])
        .args(images);
    run(command, "Load images into kind", log, ui).await
}

async fn run(command: Command, name: &str, log: &Path, ui: &Ui) -> Result<()> {
    let task = ui.task(name);
    let result = process::run(command, ui, &task, Format::Text, log).await;
    match &result {
        Ok(()) => task.pass("Ready"),
        Err(error) => task.fail(format!("{error:#}")),
    }
    result
}

pub async fn apply<K>(api: &Api<K>, resource: &K) -> Result<K>
where
    K: Clone + Debug + DeserializeOwned + Serialize + Resource<DynamicType = ()> + Send + Sync,
{
    Ok(api
        .patch(
            &resource.name_any(),
            &PatchParams::apply("ctf-local").force(),
            &Patch::Apply(resource),
        )
        .await?)
}

async fn install_crds(client: Client, ui: &Ui) -> Result<()> {
    let task = ui.task("Install operator CRDs");
    let api = Api::<CustomResourceDefinition>::all(client);
    let result = Box::pin(async {
        for crd in [
            CtfResource::crd(),
            Challenge::crd(),
            Instance::crd(),
            Flag::crd(),
            PortClaim::crd(),
            ReservationSet::crd(),
        ] {
            task.log(format!("Apply {}", crd.name_any()));
            let name = crd.name_any();
            apply(&api, &crd).await?;
            cancellable(
                ui,
                await_condition(api.clone(), &name, conditions::is_crd_established()),
            )
            .await??;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await;
    match &result {
        Ok(()) => task.pass("Schemas established"),
        Err(error) => task.fail(format!("{error:#}")),
    }
    result
}

pub async fn cancellable<T>(ui: &Ui, future: impl std::future::Future<Output = T>) -> Result<T> {
    let mut cancellation = ui.cancellation();
    ensure!(!*cancellation.borrow(), "operation cancelled");
    tokio::select! {
        result = future => Ok(result),
        _ = cancellation.changed() => bail!("operation cancelled"),
    }
}

async fn install_operator(client: Client, image: &str, ui: &Ui) -> Result<()> {
    let task = ui.task("Start local operator");
    let result = Box::pin(async {
        let namespace: Namespace = serde_json::from_value(json!({
            "apiVersion": "v1", "kind": "Namespace", "metadata": { "name": NAMESPACE }
        }))?;
        apply(&Api::all(client.clone()), &namespace).await?;
        let account: ServiceAccount = serde_json::from_value(json!({
            "apiVersion": "v1", "kind": "ServiceAccount",
            "metadata": { "name": "ctf-operator", "namespace": NAMESPACE }
        }))?;
        apply(&Api::namespaced(client.clone(), NAMESPACE), &account).await?;
        let role: ClusterRole = serde_json::from_value(json!({
            "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole",
            "metadata": { "name": "ctf-local-operator" },
            "rules": [
                { "apiGroups": ["operator.ctf.rs"], "resources": ["*"], "verbs": ["*"] },
                { "apiGroups": [""], "resources": [
                    "namespaces", "pods", "pods/log", "services", "configmaps", "secrets", "events"
                  ], "verbs": ["get", "list", "watch", "create", "update", "patch", "delete"] },
                { "apiGroups": ["apps"], "resources": ["deployments", "replicasets"],
                  "verbs": ["get", "list", "watch", "create", "update", "patch", "delete"] },
                { "apiGroups": ["networking.k8s.io"], "resources": ["networkpolicies"],
                  "verbs": ["get", "list", "watch", "create", "update", "patch", "delete"] }
            ]
        }))?;
        apply(&Api::all(client.clone()), &role).await?;
        let binding: ClusterRoleBinding = serde_json::from_value(json!({
            "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRoleBinding",
            "metadata": { "name": "ctf-local-operator" },
            "roleRef": { "apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": "ctf-local-operator" },
            "subjects": [{ "kind": "ServiceAccount", "name": "ctf-operator", "namespace": NAMESPACE }]
        }))?;
        apply(&Api::all(client.clone()), &binding).await?;
        let config: ConfigMap = serde_json::from_value(json!({
            "apiVersion": "v1", "kind": "ConfigMap",
            "metadata": { "name": "ctf-operator", "namespace": NAMESPACE },
            "data": { "operator.yaml": serde_saphyr::to_string(&json!({
                "instance": {
                    "deployment": { "imagePullPolicy": "Never", "useHostPort": false },
                    "service": { "type": "NodePort" }
                }
            }))? }
        }))?;
        apply(&Api::namespaced(client.clone(), NAMESPACE), &config).await?;
        let deployment: Deployment = serde_json::from_value(json!({
            "apiVersion": "apps/v1", "kind": "Deployment",
            "metadata": { "name": "ctf-operator", "namespace": NAMESPACE },
            "spec": {
                "replicas": 1,
                "strategy": { "type": "Recreate" },
                "selector": { "matchLabels": { "app": "ctf-operator" } },
                "template": {
                    "metadata": {
                        "labels": { "app": "ctf-operator" },
                        "annotations": { "ctf.rs/operator-image": image }
                    },
                    "spec": {
                        "serviceAccountName": "ctf-operator",
                        "securityContext": { "runAsNonRoot": true, "runAsUser": 65532, "runAsGroup": 65532 },
                        "containers": [{
                            "name": "operator", "image": image, "imagePullPolicy": "Never",
                            "args": ["operator", "--namespace", NAMESPACE, "--config", "/etc/ctf/operator.yaml"],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "capabilities": { "drop": ["ALL"] },
                                "readOnlyRootFilesystem": true
                            },
                            "volumeMounts": [{ "name": "config", "mountPath": "/etc/ctf", "readOnly": true }]
                        }],
                        "volumes": [{ "name": "config", "configMap": { "name": "ctf-operator" } }]
                    }
                }
            }
        }))?;
        let api = Api::<Deployment>::namespaced(client, NAMESPACE);
        let applied = apply(&api, &deployment).await?;
        let generation = applied.metadata.generation;
        cancellable(ui, Box::pin(await_condition(api, "ctf-operator", move |deployment: Option<&Deployment>| {
            deployment.and_then(|deployment| deployment.status.as_ref()).is_some_and(|status| {
                status.observed_generation >= generation
                    && status.updated_replicas == Some(1)
                    && status.available_replicas == Some(1)
                    && status.replicas == Some(1)
            })
        }))).await??;
        Ok::<(), anyhow::Error>(())
    })
    .await;
    match &result {
        Ok(()) => task.pass("Available"),
        Err(error) => task.fail(format!("{error:#}")),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_maps_the_entire_configured_range() -> Result<()> {
        let state = LocalCluster {
            name: "test".into(),
            ports: CtfPorts {
                start: 31000,
                end: 31002,
            },
        };
        let config = configuration(&state)?;
        let mappings = config["nodes"][0]["extraPortMappings"]
            .as_array()
            .context("missing mappings")?;
        assert_eq!(mappings.len(), 3);
        assert_eq!(mappings[2]["hostPort"], 31002);
        let patch: serde_json::Value = serde_saphyr::from_str(
            config["kubeadmConfigPatches"][0]
                .as_str()
                .context("missing patch")?,
        )?;
        assert_eq!(patch["apiServer"]["extraArgs"][0]["value"], "31000-31002");
        Ok(())
    }

    #[test]
    fn rejects_cluster_names_that_escape_state_directory() {
        LocalCluster::directory("../kind").unwrap_err();
        LocalCluster::directory("/kind").unwrap_err();
    }
}
