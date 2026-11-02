use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{ensure, Context, Result};
use serde::Serialize;
use tokio::process::Command;

use crate::{
    manifest::{FileReference, Image, LoadedChallenge, Project},
    process::{self, Format},
    ui::Ui,
};

#[derive(Debug, Serialize)]
pub struct BuiltChallenge {
    pub services: BTreeMap<String, String>,
    pub exports: BTreeMap<String, PathBuf>,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct BuildOutput {
    pub run: String,
    pub directory: PathBuf,
    pub challenges: BTreeMap<String, BuiltChallenge>,
}

pub async fn project(project: &Project, output: &Path, ui: &Ui) -> Result<BuildOutput> {
    let run = format!("{:016x}", rand::random::<u64>());
    fs::create_dir_all(output)
        .with_context(|| format!("could not create output {}", output.display()))?;
    let directory = output.canonicalize()?.join(format!("run-{run}"));
    fs::create_dir(&directory)?;
    let mut result = BuildOutput {
        run,
        directory,
        challenges: BTreeMap::new(),
    };
    for challenge in &project.challenges {
        let built = challenge_build(challenge, &result.directory, &result.run, ui).await?;
        result
            .challenges
            .insert(challenge.manifest.name.clone(), built);
    }
    fs::write(
        result.directory.join("build.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    ui.message(format!("Build outputs: {}", result.directory.display()));
    Ok(result)
}

async fn challenge_build(
    challenge: &LoadedChallenge,
    output: &Path,
    run: &str,
    ui: &Ui,
) -> Result<BuiltChallenge> {
    let name = &challenge.manifest.name;
    let root = output.join(name);
    fs::create_dir(&root)?;
    let mut services = BTreeMap::new();
    for service in &challenge.manifest.services {
        let tag = format!("ctf-local/{name}/{}:{run}", service.name);
        let image = image(
            &service.image,
            &challenge.directory,
            &tag,
            None,
            &root.join(format!("service-{}", service.name)),
            ui,
        )
        .await?;
        services.insert(service.name.clone(), image);
    }
    let mut exports = BTreeMap::new();
    for entry in &challenge.manifest.build {
        let destination = root.join(format!("build-{}", entry.name));
        fs::create_dir(&destination)?;
        image(
            &entry.image,
            &challenge.directory,
            &format!("ctf-local/{name}/{}:{run}", entry.name),
            Some(&destination),
            &root.join(format!("generator-{}", entry.name)),
            ui,
        )
        .await?;
        exports.insert(entry.name.clone(), destination);
    }
    let file_directory = root.join("files");
    fs::create_dir(&file_directory)?;
    let mut files = Vec::new();
    for value in &challenge.manifest.files {
        let source = FileReference::parse(value)?.resolve(&challenge.directory, &exports)?;
        ensure!(
            source.is_file(),
            "participant artifact is not a file: {}",
            source.display()
        );
        let destination = file_directory.join(source.file_name().context("file has no name")?);
        ensure!(!destination.exists(), "participant filenames collide");
        fs::copy(&source, &destination).with_context(|| {
            format!(
                "could not export {} to {}",
                source.display(),
                destination.display()
            )
        })?;
        files.push(destination);
    }
    Ok(BuiltChallenge {
        services,
        exports,
        files,
    })
}

pub async fn image(
    source: &Image,
    directory: &Path,
    tag: &str,
    export: Option<&Path>,
    log: &Path,
    ui: &Ui,
) -> Result<String> {
    let task = ui.task(format!("image {tag}"));
    let result = async {
        let mut command = Command::new("docker");
        match source {
            Image::Image { image } => {
                ensure!(
                    export.is_none(),
                    "filesystem generation requires a Dockerfile source"
                );
                command.args(["pull", image]);
                process::run(command, ui, &task, Format::Text, log).await?;
                Ok(image.clone())
            }
            Image::Dockerfile {
                context,
                path,
                target,
            } => {
                let context = directory.join(context).canonicalize()?;
                let dockerfile = context.join(path).canonicalize()?;
                command.args(["buildx", "build", "--progress=rawjson", "--file"]);
                command.arg(&dockerfile);
                if let Some(target) = target {
                    command.args(["--target", target]);
                }
                if let Some(destination) = export {
                    let destination = destination.to_str().context("export path must be UTF-8")?;
                    ensure!(
                        !destination.contains(','),
                        "export path cannot contain a comma"
                    );
                    command.args(["--output", &format!("type=local,dest={destination}")]);
                } else {
                    command.args(["--load", "--tag", tag]);
                }
                command.arg(context);
                process::run(command, ui, &task, Format::BuildKit, log).await?;
                Ok(tag.to_owned())
            }
        }
    }
    .await;
    match &result {
        Ok(_) => task.pass("Ready"),
        Err(error) => task.fail(format!("{error:#}")),
    }
    result
}
