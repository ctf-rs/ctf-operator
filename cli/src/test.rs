use std::{collections::BTreeMap, fs, path::Path, time::Duration};

use anyhow::{bail, ensure, Context, Result};
use ctf_core::{
    k8s::{
        Challenge as ChallengeResource, ChallengeService, ChallengeSpec, Ctf, CtfSpec, Flag,
        Instance, InstancePhase, InstanceSpec, InstanceUser,
    },
    models::ChallengePortType,
};
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::{
    api::core::v1::{Namespace, Pod},
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::{
    api::{DeleteParams, ListParams, LogParams},
    runtime::{
        wait::{await_condition, conditions},
        watcher, WatchStreamExt,
    },
    Api, Client, ResourceExt,
};
use serde::Serialize;
use tokio::process::Command;

use crate::{
    args::TestArgs,
    build::{self, BuildOutput, BuiltChallenge},
    cluster::{self, LocalCluster, NAMESPACE},
    manifest::{endpoint_prefix, FileReference, LoadedChallenge, Project, Test},
    process::{self, Format},
    ui::{Task, Ui},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Passed,
    Failed,
    NotTested,
}

#[derive(Debug, Serialize)]
struct TestResult {
    challenge: String,
    test: Option<String>,
    outcome: Outcome,
    message: String,
}

struct Runtime {
    client: Client,
    cluster: String,
    /// Address of the kind node on its dedicated Docker network.
    node: String,
    network: String,
    ctf_name: String,
    names: BTreeMap<String, String>,
    run: String,
}

pub async fn project(args: TestArgs, ui: &Ui) -> Result<()> {
    let project = Project::load(&args.project.manifest, &args.project.challenges)?;
    let output = build::project(&project, &args.project.output, ui).await?;
    let needs_runtime = project.challenges.iter().any(|challenge| {
        !challenge.manifest.services.is_empty() && !challenge.manifest.test.is_empty()
    });
    let runtime = if needs_runtime {
        let state = LocalCluster::load(&args.cluster)?;
        state.supports(&project.ctf.network.ports)?;
        Some(Runtime {
            client: state.client().await?,
            node: state.node_address().await?,
            network: state.network(),
            cluster: args.cluster,
            ctf_name: format!("run-{}", output.run),
            names: project
                .challenges
                .iter()
                .enumerate()
                .map(|(index, challenge)| {
                    (
                        challenge.manifest.name.clone(),
                        format!("ch-{}-{index}", &output.run[..8]),
                    )
                })
                .collect(),
            run: output.run.clone(),
        })
    } else {
        None
    };
    let mut results = Vec::new();
    let result = run_all(&project, &output, runtime.as_ref(), &mut results, ui).await;
    let report = output.directory.join("tests.json");
    let report_result = fs::write(&report, serde_json::to_vec_pretty(&results)?)
        .with_context(|| format!("could not save test report {}", report.display()));
    ui.message(format!(
        "Test report and complete logs: {}",
        output.directory.display()
    ));
    let cleanup = if let Some(runtime) = &runtime {
        if args.keep {
            ui.message(format!("Kept runtime resources for {}", runtime.ctf_name));
            Ok(())
        } else {
            runtime.cleanup(ui).await
        }
    } else {
        Ok(())
    };
    combine(result, combine(report_result, cleanup))
}

fn combine(result: Result<()>, cleanup: Result<()>) -> Result<()> {
    match (result, cleanup) {
        (Err(error), Err(cleanup)) => {
            Err(error.context(format!("additional finalization error: {cleanup:#}")))
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(()), result) => result,
    }
}

async fn run_all(
    project: &Project,
    output: &BuildOutput,
    runtime: Option<&Runtime>,
    results: &mut Vec<TestResult>,
    ui: &Ui,
) -> Result<()> {
    if let Some(runtime) = runtime {
        runtime.prepare(project, output, ui).await?;
    }
    let mut failures = 0;
    for challenge in &project.challenges {
        let name = &challenge.manifest.name;
        if challenge.manifest.test.is_empty() {
            ui.task(format!("test {name}"))
                .skip("No test defined; not tested");
            results.push(TestResult {
                challenge: name.clone(),
                test: None,
                outcome: Outcome::NotTested,
                message: "No test defined".to_owned(),
            });
        }
        let built = &output.challenges[name];
        for (index, test) in challenge.manifest.test.iter().enumerate() {
            ensure!(!*ui.cancellation().borrow(), "operation cancelled");
            let log = output
                .directory
                .join(name)
                .join(format!("test-{}", test.name));
            let result = run_test(
                project, challenge, built, test, index, runtime, output, &log, ui,
            )
            .await;
            let (outcome, message) = match &result {
                Ok(()) => (Outcome::Passed, "Runner completed successfully".to_owned()),
                Err(error) => {
                    failures += 1;
                    (Outcome::Failed, format!("{error:#}"))
                }
            };
            results.push(TestResult {
                challenge: name.clone(),
                test: Some(test.name.clone()),
                outcome,
                message,
            });
            if *ui.cancellation().borrow() {
                result?;
            }
        }
    }
    ensure!(
        failures == 0,
        "{failures} test(s) failed; see tests.json and runner logs"
    );
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "test orchestration receives explicit run-scoped state"
)]
async fn run_test(
    project: &Project,
    challenge: &LoadedChallenge,
    built: &BuiltChallenge,
    test: &Test,
    index: usize,
    runtime: Option<&Runtime>,
    output: &BuildOutput,
    log: &Path,
    ui: &Ui,
) -> Result<()> {
    let tag = format!(
        "ctf-local/{}/test-{}:{}",
        challenge.manifest.name, test.name, output.run
    );
    let image = build::image(
        &test.image,
        &challenge.directory,
        &tag,
        None,
        &log.with_extension("build"),
        ui,
    )
    .await?;
    let task = ui.task(format!("test {} / {}", challenge.manifest.name, test.name));
    let result = async {
        let mut environment = BTreeMap::new();
        if challenge.manifest.services.is_empty() {
            for flag in &challenge.manifest.flags {
                let resolved =
                    flag.effective_flag(&project.ctf.flag, ctf_core::models::FlagBinding::Static);
                environment.insert(format!("flag_{}", flag.name), resolved.generate());
            }
        } else {
            let runtime = runtime.context("hosted test has no local runtime")?;
            let instance = runtime.instance(challenge, index, &task, ui).await?;
            runtime
                .environment(challenge, &instance, &mut environment, &task, ui)
                .await?;
            let name = format!(
                "ctf-test-{}-{}-{index}",
                output.run, challenge.manifest.name
            );
            let result = run_container(
                &name,
                &output.run,
                Some(&runtime.network),
                &image,
                challenge,
                built,
                test,
                &environment,
                log,
                &task,
                ui,
            )
            .await;
            // Collected after the runner so service output produced during the solve is kept.
            let logs = runtime
                .save_logs(&instance, &log.with_extension("services"), &task)
                .await;
            return combine(result, logs);
        }
        let name = format!(
            "ctf-test-{}-{}-{index}",
            output.run, challenge.manifest.name
        );
        run_container(
            &name,
            &output.run,
            None,
            &image,
            challenge,
            built,
            test,
            &environment,
            log,
            &task,
            ui,
        )
        .await
    }
    .await;
    match &result {
        Ok(()) => task.pass("Runner passed"),
        Err(error) => task.fail(format!("{error:#}")),
    }
    result
}

/// Subset of `docker inspect` `.State` used to classify a finished runner.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContainerState {
    exit_code: i64,
    error: String,
    running: bool,
}

#[expect(
    clippy::too_many_arguments,
    reason = "runner configuration and lifecycle are kept in one scope"
)]
async fn run_container(
    name: &str,
    run: &str,
    network: Option<&str>,
    image: &str,
    challenge: &LoadedChallenge,
    built: &BuiltChallenge,
    test: &Test,
    environment: &BTreeMap<String, String>,
    log: &Path,
    task: &Task,
    ui: &Ui,
) -> Result<()> {
    let result = async {
        let mut command = Command::new("docker");
        command.args([
            "create",
            "--name",
            name,
            "--network",
            network.unwrap_or("none"),
            "--label",
            &format!("ctf.rs/run={run}"),
        ]);
        for (key, value) in environment {
            command.arg("--env").arg(format!("{key}={value}"));
        }
        for mount in &test.mounts {
            let source = FileReference::parse(&mount.source)?
                .resolve(&challenge.directory, &built.exports)?;
            let source = source.to_str().context("mount source must be UTF-8")?;
            let target = mount
                .target
                .to_str()
                .context("mount target must be UTF-8")?;
            ensure!(
                !source.contains(',') && !target.contains(','),
                "Docker mount paths cannot contain commas"
            );
            command
                .arg("--mount")
                .arg(format!("type=bind,src={source},dst={target},readonly"));
        }
        command.arg(image);
        process::run(
            command,
            ui,
            task,
            Format::Text,
            &log.with_extension("create"),
        )
        .await?;
        let mut start = Command::new("docker");
        start.args(["start", "--attach", name]);
        process::run(start, ui, task, Format::Text, log).await?;
        let output = Command::new("docker")
            .args(["inspect", "--format", "{{json .State}}", name])
            .output()
            .await?;
        ensure!(
            output.status.success(),
            "could not inspect test runner: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let state: ContainerState = serde_json::from_slice(&output.stdout)?;
        ensure!(
            !state.running && state.exit_code == 0 && state.error.is_empty(),
            "runner failed: exit={}, running={}, error={}",
            state.exit_code,
            state.running,
            state.error
        );
        Ok(())
    }
    .await;
    let cleanup = async {
        let listed = Command::new("docker")
            .args([
                "ps",
                "--all",
                "--filter",
                &format!("label=ctf.rs/run={run}"),
                "--format",
                "{{.Names}}",
            ])
            .output()
            .await
            .context("could not list owned test containers for cleanup")?;
        ensure!(
            listed.status.success(),
            "runner cleanup listing failed: {}",
            String::from_utf8_lossy(&listed.stderr)
        );
        if String::from_utf8(listed.stdout)?
            .lines()
            .any(|candidate| candidate == name)
        {
            let output = Command::new("docker")
                .args(["rm", "--force", name])
                .output()
                .await?;
            ensure!(
                output.status.success(),
                "could not remove runner {name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }
    .await;
    combine(result, cleanup)
}

impl Runtime {
    fn metadata(&self, name: String) -> ObjectMeta {
        ObjectMeta {
            name: Some(name),
            namespace: Some(NAMESPACE.to_owned()),
            labels: Some(BTreeMap::from([(
                "ctf.rs/run".to_owned(),
                self.run.clone(),
            )])),
            ..ObjectMeta::default()
        }
    }

    async fn prepare(&self, project: &Project, output: &BuildOutput, ui: &Ui) -> Result<()> {
        let images = project
            .challenges
            .iter()
            .filter(|challenge| !challenge.manifest.test.is_empty())
            .flat_map(|challenge| {
                output.challenges[&challenge.manifest.name]
                    .services
                    .values()
                    .cloned()
            })
            .collect::<Vec<_>>();
        cluster::load_images(
            &self.cluster,
            &images,
            &output.directory.join("kind-load"),
            ui,
        )
        .await?;
        let hosted = project.challenges.iter().filter(|challenge| {
            !challenge.manifest.services.is_empty() && !challenge.manifest.test.is_empty()
        });
        let challenges = hosted
            .clone()
            .map(|challenge| self.names[&challenge.manifest.name].clone())
            .collect();
        let ctf = Ctf {
            metadata: self.metadata(self.ctf_name.clone()),
            spec: CtfSpec {
                full_name: project.ctf.full_name.clone(),
                description: project.ctf.description.clone(),
                public: false,
                instance_duration_seconds: project.ctf.instance_duration_seconds,
                challenges,
                flag: project.ctf.flag.clone(),
                network: project.ctf.network.clone(),
                scoreboard: project.ctf.scoreboard.clone(),
                schedule: project.ctf.schedule.clone(),
            },
            status: None,
        };
        cluster::apply(&Api::namespaced(self.client.clone(), NAMESPACE), &ctf).await?;
        for challenge in hosted {
            let manifest = &challenge.manifest;
            let services = manifest
                .services
                .iter()
                .map(|service| ChallengeService {
                    name: service.name.clone(),
                    image: output.challenges[&manifest.name].services[&service.name].clone(),
                    env: service.env.clone(),
                    flags: service.flags.clone(),
                    ports: service.ports.clone(),
                    kata: service.kata.clone(),
                })
                .collect();
            let resource = ChallengeResource {
                metadata: self.metadata(self.names[&manifest.name].clone()),
                spec: ChallengeSpec {
                    full_name: manifest.full_name.clone(),
                    description: manifest.description.clone(),
                    instance_duration_seconds: manifest.instance_duration_seconds,
                    instancing: manifest.instancing,
                    categories: manifest.categories.clone(),
                    tags: manifest.tags.clone(),
                    authors: manifest.authors.clone(),
                    flags: manifest.flags.clone(),
                    services,
                    files: Vec::new(),
                },
                status: None,
            };
            cluster::apply(&Api::namespaced(self.client.clone(), NAMESPACE), &resource).await?;
        }
        Ok(())
    }

    async fn instance(
        &self,
        challenge: &LoadedChallenge,
        index: usize,
        task: &Task,
        ui: &Ui,
    ) -> Result<Instance> {
        let challenge_name = &self.names[&challenge.manifest.name];
        let api = Api::<Instance>::namespaced(self.client.clone(), NAMESPACE);
        let name = if challenge.manifest.instancing {
            let name = format!("ctf-inst-{}-{}-{index}", &self.run[..8], challenge_name);
            let instance = Instance {
                metadata: self.metadata(name.clone()),
                spec: InstanceSpec {
                    ctf: self.ctf_name.clone(),
                    challenge: challenge_name.clone(),
                    user: Some(InstanceUser {
                        id: 1,
                        username: "local-test".to_owned(),
                    }),
                    team: None,
                    duration_seconds: None,
                },
                status: None,
            };
            cluster::apply(&api, &instance).await?;
            name
        } else {
            let mut stream = watcher(api.clone(), watcher::Config::default())
                .applied_objects()
                .boxed();
            cluster::cancellable(ui, async {
                while let Some(instance) = stream.try_next().await? {
                    if instance.spec.ctf == self.ctf_name
                        && instance.spec.challenge == *challenge_name
                    {
                        return Ok::<_, anyhow::Error>(instance.name_any());
                    }
                }
                bail!("instance watcher ended before shared instance was created")
            })
            .await??
        };
        task.message(format!("Waiting for instance {name}"));
        let mut stream = watcher(
            api,
            watcher::Config::default().fields(&format!("metadata.name={name}")),
        )
        .applied_objects()
        .boxed();
        cluster::cancellable(ui, async {
            while let Some(instance) = stream.try_next().await? {
                if let Some(status) = &instance.status {
                    task.message(format!(
                        "{}: {}",
                        status.phase.description(),
                        status.message.as_deref().unwrap_or("")
                    ));
                    match status.phase {
                        InstancePhase::Ready => return Ok(instance),
                        InstancePhase::Error | InstancePhase::Terminating => {
                            bail!("instance {name}: {status:?}")
                        }
                        _ => {}
                    }
                }
            }
            bail!("instance watcher ended before {name} became ready")
        })
        .await?
    }

    async fn environment(
        &self,
        challenge: &LoadedChallenge,
        instance: &Instance,
        environment: &mut BTreeMap<String, String>,
        task: &Task,
        ui: &Ui,
    ) -> Result<()> {
        let status = instance
            .status
            .as_ref()
            .context("ready instance has no status")?;
        let namespace = status
            .namespace
            .as_deref()
            .context("ready instance has no runtime namespace")?;
        let flags = Api::<Flag>::namespaced(self.client.clone(), namespace);
        for flag in &challenge.manifest.flags {
            let resource = flags
                .get(&format!("{}-{}", instance.name_any(), flag.name))
                .await?;
            let value = resource
                .status
                .and_then(|status| status.flag)
                .context("ready instance has an unresolved flag")?;
            environment.insert(format!("flag_{}", flag.name), value);
        }
        for service in &challenge.manifest.services {
            if !service.ports.iter().any(|port| port.expose) {
                continue;
            }
            for port in service.ports.iter().filter(|port| port.expose) {
                let exposed = status
                    .endpoints
                    .iter()
                    .find(|endpoint| endpoint.service == service.name && endpoint.port == port.name)
                    .map(|endpoint| i32::from(endpoint.external_port))
                    .context("ready instance has no exposed endpoint")?;
                let prefix = endpoint_prefix(&service.name, &port.name);
                self.wait_reachable(exposed, task, ui).await?;
                let endpoint = endpoint(&port.port_type, &self.node, exposed);
                environment.insert(format!("{prefix}_HOST"), self.node.clone());
                environment.insert(format!("{prefix}_PORT"), exposed.to_string());
                environment.insert(format!("{prefix}_ENDPOINT"), endpoint);
            }
        }
        Ok(())
    }

    /// Waits until the node accepts connections on `port`. The node is reached directly on the
    /// cluster network, so an unprogrammed `NodePort` is refused rather than proxied.
    async fn wait_reachable(&self, port: i32, task: &Task, ui: &Ui) -> Result<()> {
        let address = format!("{}:{port}", self.node);
        task.message(format!("Waiting for {address}"));
        let mut delay = Duration::from_millis(100);
        cluster::cancellable(ui, async {
            loop {
                match tokio::net::TcpStream::connect(&address).await {
                    Ok(_) => return,
                    Err(error) => task.log(format!("{address} not reachable yet: {error}")),
                }
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(2));
            }
        })
        .await
    }

    async fn save_logs(&self, instance: &Instance, root: &Path, task: &Task) -> Result<()> {
        fs::create_dir_all(root)?;
        let namespace = instance
            .status
            .as_ref()
            .and_then(|status| status.namespace.as_deref())
            .context("instance has no runtime namespace")?;
        let pods = Api::<Pod>::namespaced(self.client.clone(), namespace);
        for pod in pods.list(&ListParams::default()).await? {
            let name = pod.name_any();
            for container in pod
                .spec
                .as_ref()
                .into_iter()
                .flat_map(|spec| &spec.containers)
            {
                let logs = pods
                    .logs(
                        &name,
                        &LogParams {
                            container: Some(container.name.clone()),
                            ..LogParams::default()
                        },
                    )
                    .await?;
                let path = root.join(format!("{name}-{}.log", container.name));
                fs::write(&path, logs)?;
                task.log(format!("Service log: {}", path.display()));
            }
        }
        Ok(())
    }

    async fn cleanup(&self, ui: &Ui) -> Result<()> {
        let task = ui.task("Clean owned test runtime");
        let result = async {
            let challenges = Api::<ChallengeResource>::namespaced(self.client.clone(), NAMESPACE);
            for name in self.names.values() {
                if challenges.get_opt(name).await?.is_some() {
                    challenges.delete(name, &DeleteParams::default()).await?;
                }
            }
            let api = Api::<Instance>::namespaced(self.client.clone(), NAMESPACE);
            for instance in api.list(&ListParams::default()).await? {
                let owned = instance.spec.ctf == self.ctf_name
                    && self
                        .names
                        .values()
                        .any(|name| *name == instance.spec.challenge);
                if !owned {
                    continue;
                }
                let name = instance.name_any();
                api.delete(&name, &DeleteParams::default()).await?;
                let uid = instance.uid().context("instance has no UID")?;
                await_condition(api.clone(), &name, conditions::is_deleted(&uid)).await?;
                let namespaces = Api::<Namespace>::all(self.client.clone());
                if let Some(namespace) = instance
                    .status
                    .as_ref()
                    .and_then(|status| status.namespace.as_deref())
                {
                    while namespaces.get_opt(namespace).await?.is_some() {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
            let ctfs = Api::<Ctf>::namespaced(self.client.clone(), NAMESPACE);
            if ctfs.get_opt(&self.ctf_name).await?.is_some() {
                ctfs.delete(&self.ctf_name, &DeleteParams::default())
                    .await?;
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;
        match &result {
            Ok(()) => task.pass("Removed"),
            Err(error) => task.fail(format!("{error:#}")),
        }
        result
    }
}

fn endpoint(kind: &ChallengePortType, host: &str, port: i32) -> String {
    let scheme = match kind {
        ChallengePortType::HTTP => "http://",
        ChallengePortType::HTTPS => "https://",
        ChallengePortType::SSH => "ssh://",
        ChallengePortType::NETCAT | ChallengePortType::STTY => "",
    };
    format!("{scheme}{host}:{port}")
}
