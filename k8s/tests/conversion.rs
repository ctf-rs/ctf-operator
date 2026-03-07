use std::{collections::HashSet, path::PathBuf};

use ctf_core::{
    Challenge as CoreChallenge, ChallengeContainer, ChallengeFlag, Ctf as CoreCtf, CtfFlag,
    CtfNetwork, CtfPorts, CtfRegistry, CtfStorage, CtfStorageKind, FlagFormat,
};
use ctf_k8s::{
    challenge_from_core, ctf_from_core, instance_from_refs, ChallengeFileRef, ConversionError,
};

fn sample_ctf() -> CoreCtf {
    CoreCtf {
        name: "sample-ctf".to_owned(),
        full_name: "Sample CTF".to_owned(),
        description: Some("Sample description".to_owned()),
        registry: Some(CtfRegistry {
            url: "registry.local:5000/ctf".to_owned(),
        }),
        storage: Some(CtfStorage {
            kind: CtfStorageKind::S3,
            bucket: "challenge-files".to_owned(),
            endpoint: "http://rgw.local".to_owned(),
            region: None,
            path_style: true,
            prefix: None,
        }),
        challenges: Vec::new(),
        flag: CtfFlag {
            prefix: "ctf".to_owned(),
            format: FlagFormat::Curly,
            random: None,
        },
        network: CtfNetwork {
            ports: CtfPorts {
                start: 40000,
                end: 41000,
            },
            ip: None,
        },
    }
}

fn sample_challenge() -> CoreChallenge {
    CoreChallenge {
        name: "baby-pwn".to_owned(),
        full_name: "Baby Pwn".to_owned(),
        description: Some("Warmup pwn challenge".to_owned()),
        categories: HashSet::new(),
        tags: HashSet::new(),
        authors: HashSet::new(),
        files: vec![PathBuf::from("./dist/challenge.zip")],
        flags: vec![ChallengeFlag {
            name: "main".to_owned(),
            body: "flag-body".to_owned(),
            prefix: None,
            format: None,
            random: None,
            options: Default::default(),
        }],
        containers: vec![ChallengeContainer {
            name: "service".to_owned(),
            image: ctf_core::ContainerImage::Dockerfile {
                path: PathBuf::from("./Dockerfile"),
            },
            flags: Vec::new(),
            ports: Vec::new(),
        }],
    }
}

#[test]
fn challenge_conversion_rewrites_image_and_files_and_uses_metadata_name() {
    let ctf = sample_ctf();
    let challenge = sample_challenge();

    let converted = challenge_from_core(&ctf, &challenge, Some("ctf".to_owned())).unwrap();

    assert_eq!(converted.metadata.name.as_deref(), Some("baby-pwn"));
    assert_eq!(
        converted.spec.containers[0].image,
        "registry.local:5000/ctf/baby-pwn:service"
    );
    assert_eq!(
        converted.spec.files,
        vec![ChallengeFileRef::S3 {
            bucket: "challenge-files".to_owned(),
            key: "challenge/baby-pwn/challenge.zip".to_owned()
        }]
    );

    let value = serde_json::to_value(&converted).unwrap();
    assert!(value["spec"].get("name").is_none());
}

#[test]
fn challenge_conversion_uses_storage_prefix_when_defined() {
    let mut ctf = sample_ctf();
    ctf.storage.as_mut().unwrap().prefix = Some("assets/team-a".to_owned());
    let challenge = sample_challenge();

    let converted = challenge_from_core(&ctf, &challenge, None).unwrap();

    assert_eq!(
        converted.spec.files,
        vec![ChallengeFileRef::S3 {
            bucket: "challenge-files".to_owned(),
            key: "assets/team-a/challenge/baby-pwn/challenge.zip".to_owned()
        }]
    );
}

#[test]
fn challenge_conversion_requires_registry() {
    let mut ctf = sample_ctf();
    ctf.registry = None;
    let challenge = sample_challenge();

    let error = challenge_from_core(&ctf, &challenge, None).unwrap_err();
    assert!(matches!(error, ConversionError::MissingRegistry));
}

#[test]
fn challenge_conversion_requires_storage() {
    let mut ctf = sample_ctf();
    ctf.storage = None;
    let challenge = sample_challenge();

    let error = challenge_from_core(&ctf, &challenge, None).unwrap_err();
    assert!(matches!(error, ConversionError::MissingStorage));
}

#[test]
fn ctf_conversion_moves_name_to_metadata_and_sets_challenges() {
    let ctf = sample_ctf();
    let challenge = sample_challenge();

    let converted = ctf_from_core(&ctf, &[challenge], Some("ctf".to_owned()));

    assert_eq!(converted.metadata.name.as_deref(), Some("sample-ctf"));
    assert_eq!(converted.spec.full_name, "Sample CTF");
    assert_eq!(converted.spec.challenges.len(), 1);
    assert_eq!(converted.spec.challenges[0], "baby-pwn");

    let value = serde_json::to_value(&converted).unwrap();
    assert!(value["spec"].get("name").is_none());
}

#[test]
fn instance_conversion_contains_ctf_and_challenge_refs() {
    let instance = instance_from_refs("inst-1", "sample-ctf", "baby-pwn", Some("ctf".to_owned()));

    assert_eq!(instance.metadata.name.as_deref(), Some("inst-1"));
    assert_eq!(instance.spec.ctf, "sample-ctf");
    assert_eq!(instance.spec.challenge, "baby-pwn");
}
