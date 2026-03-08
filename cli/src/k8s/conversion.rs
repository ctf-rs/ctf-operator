use crate::models::{Challenge as CoreChallenge, Ctf as CoreCtf, CtfStorageKind};

use crate::k8s::{
    Challenge, ChallengeContainer, ChallengeFileRef, ChallengeSpec, ConversionError,
    ConversionResult, Ctf, CtfSpec, Instance, InstanceSpec,
};

pub fn challenge_from_core(
    ctf: &CoreCtf,
    challenge: &CoreChallenge,
    namespace: Option<String>,
) -> ConversionResult<Challenge> {
    let registry = ctf
        .registry
        .as_ref()
        .ok_or(ConversionError::MissingRegistry)?;

    let storage = ctf
        .storage
        .as_ref()
        .ok_or(ConversionError::MissingStorage)?;

    if !matches!(storage.kind, CtfStorageKind::S3) {
        return Err(ConversionError::UnsupportedStorageType);
    }

    let registry_base = registry.url.trim_end_matches('/');
    let key_prefix = normalize_prefix(storage.prefix.as_deref());

    let containers = challenge
        .containers
        .iter()
        .map(|container| ChallengeContainer {
            name: container.name.clone(),
            image: format!("{registry_base}/{}:{}", challenge.name, container.name),
            flags: container.flags.clone(),
            ports: container.ports.clone(),
        })
        .collect::<Vec<_>>();

    let files = challenge
        .files
        .iter()
        .map(|path| {
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| ConversionError::InvalidChallengeFilePath { path: path.clone() })?;

            let key = format!("{}challenge/{}/{}", key_prefix, challenge.name, file_name);
            Ok(ChallengeFileRef::S3 {
                bucket: storage.bucket.clone(),
                key,
            })
        })
        .collect::<ConversionResult<Vec<_>>>()?;

    let mut resource = Challenge::new(
        &challenge.name,
        ChallengeSpec {
            full_name: challenge.full_name.clone(),
            description: challenge.description.clone(),
            categories: challenge.categories.clone(),
            tags: challenge.tags.clone(),
            authors: challenge.authors.clone(),
            conditions: Default::default(),
            flags: challenge.flags.clone(),
            containers,
            files,
        },
    );
    resource.metadata.namespace = namespace;

    Ok(resource)
}

pub fn ctf_from_core(
    ctf: &CoreCtf,
    challenges: &[CoreChallenge],
    namespace: Option<String>,
) -> Ctf {
    let challenge_refs = challenges
        .iter()
        .map(|challenge| challenge.name.clone())
        .collect::<Vec<_>>();

    let mut resource = Ctf::new(
        &ctf.name,
        CtfSpec {
            full_name: ctf.full_name.clone(),
            description: ctf.description.clone(),
            conditions: Default::default(),
            challenges: challenge_refs,
            flag: ctf.flag.clone(),
            network: ctf.network.clone(),
        },
    );
    resource.metadata.namespace = namespace;
    resource
}

pub fn instance_from_refs(
    instance_name: &str,
    ctf_name: &str,
    challenge_name: &str,
    namespace: Option<String>,
) -> Instance {
    let mut resource = Instance::new(
        instance_name,
        InstanceSpec {
            ctf: ctf_name.to_owned(),
            challenge: challenge_name.to_owned(),
            conditions: Default::default(),
        },
    );
    resource.metadata.namespace = namespace;
    resource
}

fn normalize_prefix(prefix: Option<&str>) -> String {
    match prefix {
        Some(value) if !value.trim().is_empty() => {
            let trimmed = value.trim().trim_matches('/');
            if trimmed.is_empty() {
                String::new()
            } else {
                format!("{trimmed}/")
            }
        }
        _ => String::new(),
    }
}
