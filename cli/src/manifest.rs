use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, ensure, Context, Result};
use ctf_core::models::{
    Challenge as DomainChallenge, ChallengeCategory, ChallengeFlag, ChallengeService, ContainerEnv,
    ContainerFlag, ContainerImage, ContainerKata, ContainerPort, Ctf,
};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub name: String,
    #[serde(rename = "fullName")]
    pub full_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub categories: HashSet<ChallengeCategory>,
    #[serde(default)]
    pub tags: HashSet<String>,
    #[serde(default)]
    pub authors: HashSet<String>,
    #[serde(default)]
    pub flags: Vec<ChallengeFlag>,
    #[serde(default)]
    pub instancing: bool,
    #[serde(default, rename = "instanceDurationSeconds")]
    pub instance_duration_seconds: Option<u32>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub services: Vec<Service>,
    #[serde(default)]
    pub build: Vec<Build>,
    #[serde(default)]
    pub test: Vec<Test>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub name: String,
    pub image: Image,
    #[serde(default)]
    pub env: Vec<ContainerEnv>,
    #[serde(default)]
    pub flags: Vec<ContainerFlag>,
    #[serde(default)]
    pub ports: Vec<ContainerPort>,
    #[serde(default)]
    pub kata: Option<ContainerKata>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub name: String,
    pub image: Image,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Test {
    pub name: String,
    pub image: Image,
    #[serde(default)]
    pub mounts: Vec<Mount>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mount {
    pub source: String,
    pub target: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Image {
    Dockerfile {
        #[serde(default = "default_context")]
        context: PathBuf,
        path: PathBuf,
        #[serde(default)]
        target: Option<String>,
    },
    Image {
        image: String,
    },
}

fn default_context() -> PathBuf {
    PathBuf::from(".")
}

#[derive(Debug)]
pub struct LoadedChallenge {
    pub directory: PathBuf,
    pub manifest: Challenge,
}

#[derive(Debug)]
pub struct Project {
    pub ctf: Ctf,
    /// Names of every challenge in the event, in manifest order.
    pub names: Vec<String>,
    /// Selected challenges.
    pub challenges: Vec<LoadedChallenge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileReference {
    Host(PathBuf),
    Build { name: String, path: PathBuf },
}

impl FileReference {
    pub fn parse(value: &str) -> Result<Self> {
        if !value.starts_with("build:") {
            return Ok(Self::Host(PathBuf::from(value)));
        }
        let url = Url::parse(value).context("invalid build file reference")?;
        ensure!(url.scheme() == "build", "unsupported file reference scheme");
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "build references must not contain credentials, ports, queries, or fragments"
        );
        let name = url
            .host_str()
            .context("build reference has no build name")?;
        let decoded = percent_decode_str(url.path())
            .decode_utf8()
            .context("build path is not UTF-8")?;
        let path = PathBuf::from(decoded.trim_start_matches('/'));
        ensure!(
            !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
            "build reference must select a path within the exported filesystem"
        );
        Ok(Self::Build {
            name: name.to_owned(),
            path,
        })
    }

    pub fn resolve(
        &self,
        directory: &Path,
        exports: &BTreeMap<String, PathBuf>,
    ) -> Result<PathBuf> {
        match self {
            Self::Host(path) => directory.join(path).canonicalize().with_context(|| {
                format!("input does not exist: {}", directory.join(path).display())
            }),
            Self::Build { name, path } => {
                let root = exports
                    .get(name)
                    .with_context(|| format!("build {name} has no output"))?;
                let root = root.canonicalize()?;
                let selected = root
                    .join(path)
                    .canonicalize()
                    .with_context(|| format!("build {name} did not produce {}", path.display()))?;
                ensure!(
                    selected.starts_with(&root),
                    "generated input escapes build {name}"
                );
                Ok(selected)
            }
        }
    }
}

impl Image {
    pub fn validate(&self, directory: &Path) -> Result<()> {
        match self {
            Self::Dockerfile {
                context,
                path,
                target,
            } => {
                let context = directory.join(context);
                ensure!(
                    context.is_dir(),
                    "missing Docker context {}",
                    context.display()
                );
                let dockerfile = context.join(path);
                ensure!(
                    dockerfile.is_file(),
                    "missing Dockerfile {}",
                    dockerfile.display()
                );
                if let Some(target) = target {
                    ensure!(!target.is_empty(), "Docker target must not be empty");
                }
            }
            Self::Image { image } => ensure!(!image.trim().is_empty(), "image reference is empty"),
        }
        Ok(())
    }
}

impl Project {
    pub fn load(path: &Path, selected: &[String]) -> Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("cannot read {}", path.display()))?;
        let root = path.parent().context("event manifest has no parent")?;
        let ctf: Ctf = serde_saphyr::from_str(&fs::read_to_string(&path)?)
            .with_context(|| format!("invalid event manifest {}", path.display()))?;
        ctf.validate_strict()
            .context("invalid event configuration")?;
        let mut challenges = Vec::new();
        let mut names = HashSet::new();
        let mut ordered = Vec::new();
        for relative in &ctf.challenges {
            let manifest_path = root.join(relative);
            let manifest_path = manifest_path
                .canonicalize()
                .with_context(|| format!("missing challenge {}", manifest_path.display()))?;
            let directory = manifest_path
                .parent()
                .context("challenge has no parent")?
                .to_owned();
            let manifest: Challenge = serde_saphyr::from_str(&fs::read_to_string(&manifest_path)?)
                .with_context(|| format!("invalid challenge {}", manifest_path.display()))?;
            ensure!(
                names.insert(manifest.name.clone()),
                "duplicate challenge {}",
                manifest.name
            );
            ordered.push(manifest.name.clone());
            manifest
                .validate(&directory)
                .with_context(|| format!("invalid challenge {}", manifest.name))?;
            if selected.is_empty() || selected.contains(&manifest.name) {
                challenges.push(LoadedChallenge {
                    directory,
                    manifest,
                });
            }
        }
        for name in selected {
            ensure!(names.contains(name), "unknown challenge selection {name}");
        }
        ensure!(!challenges.is_empty(), "no challenges selected");
        Ok(Self {
            ctf,
            names: ordered,
            challenges,
        })
    }
}

impl Challenge {
    pub fn domain(&self) -> DomainChallenge {
        DomainChallenge {
            name: self.name.clone(),
            full_name: self.full_name.clone(),
            description: self.description.clone(),
            categories: self.categories.clone(),
            tags: self.tags.clone(),
            authors: self.authors.clone(),
            instance_duration_seconds: self.instance_duration_seconds,
            instancing: self.instancing,
            files: self.files.iter().map(PathBuf::from).collect(),
            flags: self.flags.clone(),
            services: self
                .services
                .iter()
                .map(|service| ChallengeService {
                    name: service.name.clone(),
                    image: ContainerImage::Image {
                        image: "resolved".into(),
                    },
                    env: service.env.clone(),
                    flags: service.flags.clone(),
                    ports: service.ports.clone(),
                    kata: service.kata.clone(),
                })
                .collect(),
        }
    }

    fn validate(&self, directory: &Path) -> Result<()> {
        self.domain()
            .validate_strict()
            .context("invalid challenge metadata")?;
        validate_names(self.build.iter().map(|entry| entry.name.as_str()), "build")?;
        validate_names(self.test.iter().map(|entry| entry.name.as_str()), "test")?;
        let mut environment_names = HashSet::new();
        for service in &self.services {
            service.image.validate(directory)?;
            for port in &service.ports {
                let key = endpoint_prefix(&service.name, &port.name);
                ensure!(
                    environment_names.insert(key),
                    "colliding endpoint environment names"
                );
            }
        }
        for entry in &self.build {
            entry.image.validate(directory)?;
        }
        for entry in &self.test {
            entry.image.validate(directory)?;
            let mut targets = HashSet::new();
            for mount in &entry.mounts {
                ensure!(mount.target.is_absolute(), "mount target must be absolute");
                ensure!(targets.insert(&mount.target), "duplicate mount target");
                self.validate_reference(&mount.source, directory, false)?;
            }
        }
        let mut file_names = HashSet::new();
        for file in &self.files {
            self.validate_reference(file, directory, true)?;
            let name = match FileReference::parse(file)? {
                FileReference::Host(path) | FileReference::Build { path, .. } => path
                    .file_name()
                    .context("file reference has no filename")?
                    .to_owned(),
            };
            ensure!(file_names.insert(name), "participant filenames collide");
        }
        for flag in &self.flags {
            ensure!(
                flag.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "flag names must be safe lowercase environment suffixes: {}",
                flag.name
            );
        }
        Ok(())
    }

    fn validate_reference(&self, value: &str, directory: &Path, file_only: bool) -> Result<()> {
        match FileReference::parse(value)? {
            FileReference::Host(path) => {
                let path = directory.join(path);
                ensure!(path.exists(), "missing input {}", path.display());
                if file_only {
                    ensure!(
                        path.is_file(),
                        "participant input is not a file: {}",
                        path.display()
                    );
                }
            }
            FileReference::Build { name, .. } => {
                ensure!(
                    self.build.iter().any(|entry| entry.name == name),
                    "unknown build {name}"
                );
            }
        }
        Ok(())
    }
}

fn validate_names<'a>(names: impl Iterator<Item = &'a str>, kind: &str) -> Result<()> {
    let mut seen = HashSet::new();
    for name in names {
        ensure!(
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "invalid {kind} name {name}"
        );
        if !seen.insert(name) {
            bail!("duplicate {kind} name {name}");
        }
    }
    Ok(())
}

#[must_use]
pub fn endpoint_prefix(service: &str, port: &str) -> String {
    format!(
        "SERVICE_{}_{}",
        service.replace('-', "_"),
        port.replace('-', "_")
    )
    .to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_reference_uses_result_root_and_decodes_paths() -> Result<()> {
        assert_eq!(
            FileReference::parse("build://prepare/other/data%20file.bin")?,
            FileReference::Build {
                name: "prepare".into(),
                path: "other/data file.bin".into()
            }
        );
        Ok(())
    }

    #[test]
    fn malformed_generated_references_are_rejected() {
        for value in [
            "build:///out/file",
            "build://name/",
            "build://name/file?query",
            "build://user@name/file",
        ] {
            FileReference::parse(value).expect_err(value);
        }
    }

    #[test]
    fn image_source_rejects_misspelled_fields() {
        serde_json::from_str::<Image>(r#"{"type":"dockerfile","path":"Dockerfile","contex":"."}"#)
            .unwrap_err();
    }
}
