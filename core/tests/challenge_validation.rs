use ctf_core::{Challenge, ContainerImage, ValidationError};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn baseline_challenge_yaml() -> &'static str {
    include_str!("../../examples/challenge-test.yaml")
}

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
fn baseline_challenge_yaml_parses_and_validates() {
    let challenge: Challenge = serde_saphyr::from_str(baseline_challenge_yaml())
        .expect("baseline challenge-test.yaml should parse");

    challenge
        .validate_strict()
        .expect("baseline challenge-test.yaml should validate");

    assert_eq!(challenge.name, "challenge-test");
    assert_eq!(challenge.full_name, "Challenge Test");
    assert_eq!(challenge.flags.len(), 1);
    assert_eq!(challenge.containers.len(), 1);
}

#[test]
fn baseline_challenge_yaml_validates_paths_relative_to_examples_dir() {
    let challenge: Challenge = serde_saphyr::from_str(baseline_challenge_yaml())
        .expect("baseline challenge-test.yaml should parse");

    let challenge_yaml_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/challenge-test.yaml");

    challenge
        .validate_strict_at(&challenge_yaml_path)
        .expect("baseline challenge paths should resolve and validate");
}

#[test]
fn baseline_challenge_container_dockerfile_parses() {
    let challenge: Challenge = serde_saphyr::from_str(baseline_challenge_yaml())
        .expect("baseline challenge-test.yaml should parse");

    let container = challenge
        .containers
        .iter()
        .find(|container| container.name == "test")
        .expect("baseline should contain test container");

    match &container.image {
        ContainerImage::Dockerfile { path } => {
            assert_eq!(path.to_string_lossy(), "challenge-test/Dockerfile");
        }
        _ => panic!("test container should use dockerfile image type"),
    }
}

#[test]
fn challenge_duplicate_flag_name_returns_typed_error() {
    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
flags:
  - name: flag
    body: fake_flag
  - name: flag
    body: fake_flag2
containers:
  - name: test
    image:
      type: image
      image: alpine:latest
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict()
        .expect_err("duplicate flag names must fail");

    assert_eq!(err, ValidationError::DuplicateFlagName);
}

#[test]
fn challenge_duplicate_container_name_returns_typed_error() {
    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
flags:
  - name: flag
    body: fake_flag
containers:
  - name: test
    image:
      type: image
      image: alpine:latest
  - name: test
    image:
      type: image
      image: alpine:latest
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict()
        .expect_err("duplicate container names must fail");

    assert_eq!(err, ValidationError::DuplicateContainerName);
}

#[test]
fn challenge_unknown_container_flag_reference_returns_typed_error() {
    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
flags:
  - name: flag
    body: fake_flag
containers:
  - name: test
    image:
      type: image
      image: alpine:latest
    flags:
      - name: unknown
        env: FLAG
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict()
        .expect_err("unknown container flag reference must fail");

    assert_eq!(err, ValidationError::UnknownContainerFlagReference);
}

#[test]
fn challenge_port_names_must_be_unique_across_all_containers() {
    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
flags:
  - name: flag
    body: fake_flag
containers:
  - name: c1
    image:
      type: image
      image: alpine:latest
    ports:
      - name: p1
        type: netcat
        containerPort: 1337
  - name: c2
    image:
      type: image
      image: alpine:latest
    ports:
      - name: p1
        type: netcat
        containerPort: 1338
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict()
        .expect_err("duplicate challenge port names across containers must fail");

    assert_eq!(err, ValidationError::DuplicateChallengePortName);
}

#[test]
fn container_env_names_must_be_unique_per_container() {
    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
flags:
  - name: flag1
    body: fake_flag1
  - name: flag2
    body: fake_flag2
containers:
  - name: test
    image:
      type: image
      image: alpine:latest
    flags:
      - name: flag1
        env: FLAG
      - name: flag2
        env: FLAG
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict()
        .expect_err("duplicate env names in a container must fail");

    assert_eq!(err, ValidationError::DuplicateContainerEnv);
}

#[test]
fn challenge_files_paths_must_exist() {
    let dir = make_temp_dir("challenge-files-missing");
    let challenge_yaml_path = dir.join("challenge.yaml");

    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
files:
  - files/missing.txt
flags:
  - name: flag
    body: fake_flag
containers:
  - name: test
    image:
      type: image
      image: alpine:latest
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict_at(&challenge_yaml_path)
        .expect_err("missing challenge file must fail");

    assert!(matches!(err, ValidationError::FileNotFound { .. }));
}

#[test]
fn challenge_files_paths_must_not_escape_base_dir() {
    let dir = make_temp_dir("challenge-files-invalid");
    let challenge_yaml_path = dir.join("challenge.yaml");

    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
files:
  - ../outside.txt
flags:
  - name: flag
    body: fake_flag
containers:
  - name: test
    image:
      type: image
      image: alpine:latest
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict_at(&challenge_yaml_path)
        .expect_err("path traversal must fail");

    assert!(matches!(err, ValidationError::InvalidPath { .. }));
}

#[test]
fn challenge_dockerfile_paths_must_exist() {
    let dir = make_temp_dir("challenge-dockerfile-missing");
    let challenge_yaml_path = dir.join("challenge.yaml");

    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
flags:
  - name: flag
    body: fake_flag
containers:
  - name: test
    image:
      type: dockerfile
      path: Dockerfile
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict_at(&challenge_yaml_path)
        .expect_err("missing dockerfile must fail");

    assert!(matches!(err, ValidationError::FileNotFound { .. }));
}

#[test]
fn challenge_dockerfile_paths_validate_when_file_exists() {
    let dir = make_temp_dir("challenge-dockerfile-valid");
    let challenge_yaml_path = dir.join("challenge.yaml");
    write_file(&dir.join("Dockerfile"), "FROM alpine:latest\n");

    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
flags:
  - name: flag
    body: fake_flag
containers:
  - name: test
    image:
      type: dockerfile
      path: Dockerfile
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    challenge
        .validate_strict_at(&challenge_yaml_path)
        .expect("dockerfile path should validate");
}

#[test]
fn challenge_files_must_have_unique_file_names() {
    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
files:
  - dir1/flag.txt
  - dir2/flag.txt
flags:
  - name: flag
    body: fake_flag
containers:
  - name: test
    image:
      type: image
      image: alpine:latest
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict()
        .expect_err("duplicate file names should fail");

    assert_eq!(err, ValidationError::DuplicateChallengeFileName);
}

#[test]
fn challenge_can_have_at_most_three_files() {
    let yaml = r#"
name: challenge-test
fullName: Challenge Test
categories:
  - misc
files:
  - file1.txt
  - file2.txt
  - file3.txt
  - file4.txt
flags:
  - name: flag
    body: fake_flag
containers:
  - name: test
    image:
      type: image
      image: alpine:latest
"#;

    let challenge: Challenge = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = challenge
        .validate_strict()
        .expect_err("more than three files should fail");

    assert_eq!(err, ValidationError::TooManyChallengeFiles);
}
