use ctf_core::{Ctf, ValidationError};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn make_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock drift")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ctf-core-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("failed to create temp test directory");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("failed to create parent directory");
    }
    fs::write(path, content).expect("failed to write test file");
}

#[test]
fn ctf_valid_yaml_parses_and_validates() {
    let yaml = r#"
name: ctf
fullName: CTF Example
registry:
  url: registry.ctf.rs/playground
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#;

    let ctf: Ctf = serde_saphyr::from_str(yaml).expect("yaml should parse");
    ctf.validate_strict().expect("ctf should validate");
}

#[test]
fn ctf_without_registry_is_valid() {
    let yaml = r#"
name: ctf
fullName: CTF Example
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#;

    let ctf: Ctf = serde_saphyr::from_str(yaml).expect("yaml should parse");
    ctf.validate_strict()
        .expect("ctf without registry should validate");
}

#[test]
fn ctf_invalid_port_range_returns_typed_error() {
    let yaml = r#"
name: ctf
fullName: CTF
registry:
  url: registry.ctf.rs/playground
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 1000
"#;

    let ctf: Ctf = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = ctf
        .validate_strict()
        .expect_err("start > end must fail strict validation");

    assert_eq!(err, ValidationError::ServiceReservationStartGreaterThanEnd);
}

#[test]
fn ctf_challenge_paths_are_validated_relative_to_ctf_yaml() {
    let dir = make_temp_dir("ctf-challenges-valid");
    let ctf_yaml_path = dir.join("ctf.yaml");
    let challenge_path = dir.join("challenges/challenge.yaml");
    write_file(
        &challenge_path,
        "name: challenge\nfullName: Challenge\ncategories: [misc]\nflags: []\ncontainers: []\n",
    );

    let yaml = r#"
name: ctf
fullName: CTF Example
registry:
  url: registry.ctf.rs/playground
challenges:
  - challenges/challenge.yaml
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#;

    let ctf: Ctf = serde_saphyr::from_str(yaml).expect("yaml should parse");
    ctf.validate_strict_at(&ctf_yaml_path)
        .expect("ctf challenge path should validate");
}

#[test]
fn ctf_challenge_path_must_exist() {
    let dir = make_temp_dir("ctf-challenges-missing");
    let ctf_yaml_path = dir.join("ctf.yaml");

    let yaml = r#"
name: ctf
fullName: CTF Example
registry:
  url: registry.ctf.rs/playground
challenges:
  - challenges/missing.yaml
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#;

    let ctf: Ctf = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = ctf
        .validate_strict_at(&ctf_yaml_path)
        .expect_err("missing challenge yaml should fail");

    assert!(matches!(err, ValidationError::FileNotFound { .. }));
}

#[test]
fn ctf_challenge_path_must_not_escape_base_dir() {
    let dir = make_temp_dir("ctf-challenges-invalid-path");
    let ctf_yaml_path = dir.join("ctf.yaml");

    let yaml = r#"
name: ctf
fullName: CTF Example
registry:
  url: registry.ctf.rs/playground
challenges:
  - ../outside.yaml
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#;

    let ctf: Ctf = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = ctf
        .validate_strict_at(&ctf_yaml_path)
        .expect_err("path traversal must fail");

    assert!(matches!(err, ValidationError::InvalidPath { .. }));
}

#[test]
fn ctf_registry_url_accepts_valid_values() {
    let values = [
        "registry.ctf.rs/playground",
        "registry.ctf.rs",
        "registry.ctf.rs:5000/playground",
        "registry.ctf.rs/team-alpha/challs",
    ];

    for value in values {
        let yaml = format!(
            r#"
name: ctf
fullName: CTF Example
registry:
  url: {value}
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#
        );

        let ctf: Ctf = serde_saphyr::from_str(&yaml).expect("yaml should parse");
        ctf.validate_strict()
            .unwrap_or_else(|err| panic!("expected valid registry url '{value}', got: {err}"));
    }
}

#[test]
fn ctf_registry_url_rejects_invalid_values() {
    let values = [
        "https://registry.ctf.rs/playground",
        "registry.ctf.rs/playground/",
        "Registry.ctf.rs/playground",
        "registry ctf rs/playground",
        "registry.ctf.rs/Playground",
    ];

    for value in values {
        let yaml = format!(
            r#"
name: ctf
fullName: CTF Example
registry:
  url: {value}
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#
        );

        let ctf: Ctf = serde_saphyr::from_str(&yaml).expect("yaml should parse");
        let err = ctf
            .validate_strict()
            .expect_err("invalid registry url should fail");
        assert!(
            matches!(err, ValidationError::Garde(_)),
            "expected garde error for value '{value}', got: {err}"
        );
    }
}

#[test]
fn ctf_storage_s3_is_valid() {
    let yaml = r#"
name: ctf
fullName: CTF Example
storage:
  type: s3
  bucket: cyberhero-playground-storage
  endpoint: https://rook-ceph-rgw.rook-ceph.svc
  region: us-east-1
  prefix: playground
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#;

    let ctf: Ctf = serde_saphyr::from_str(yaml).expect("yaml should parse");
    ctf.validate_strict()
        .expect("ctf with s3 storage should validate");
}

#[test]
fn ctf_storage_s3_without_region_is_valid() {
    let yaml = r#"
name: ctf
fullName: CTF Example
storage:
  type: s3
  bucket: cyberhero-playground-storage
  endpoint: https://rook-ceph-rgw.rook-ceph.svc
  prefix: playground
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#;

    let ctf: Ctf = serde_saphyr::from_str(yaml).expect("yaml should parse");
    ctf.validate_strict()
        .expect("ctf with s3 storage and no region should validate");
}

#[test]
fn ctf_storage_bucket_must_match_s3_bucket_pattern() {
    let yaml = r#"
name: ctf
fullName: CTF Example
storage:
  type: s3
  bucket: Invalid_Bucket
  endpoint: https://rook-ceph-rgw.rook-ceph.svc
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#;

    let ctf: Ctf = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = ctf
        .validate_strict()
        .expect_err("invalid bucket should fail");

    assert!(matches!(err, ValidationError::Garde(_)));
}

#[test]
fn ctf_storage_kind_only_supports_s3_for_now() {
    let yaml = r#"
name: ctf
fullName: CTF Example
storage:
  type: local
  bucket: cyberhero-playground-storage
flag:
  prefix: ctf
  format: curly
network:
  ports:
    start: 2000
    end: 3000
"#;

    let parsed = serde_saphyr::from_str::<Ctf>(yaml);
    assert!(
        parsed.is_err(),
        "unsupported storage kind should fail parsing"
    );
}
