use ctf_core::{Challenge, ContainerImage, ValidationError};

fn baseline_challenge_yaml() -> &'static str {
    include_str!("../../examples/challenge-test.yaml")
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
