use ctf_core::{Ctf, ValidationError};

#[test]
fn ctf_valid_yaml_parses_and_validates() {
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
    ctf.validate_strict().expect("ctf should validate");
}

#[test]
fn ctf_invalid_port_range_returns_typed_error() {
    let yaml = r#"
name: ctf
fullName: CTF
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
