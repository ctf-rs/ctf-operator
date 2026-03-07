use ctf_core::{Flag, ValidationError};

#[test]
fn flag_valid_yaml_parses_and_validates() {
    let yaml = r#"
prefix: ctf
format: curly
body: fake_flag
random:
  len: 8
  special: "!@"
"#;

    let flag: Flag = serde_saphyr::from_str(yaml).expect("yaml should parse");
    flag.validate_strict().expect("flag should validate");
}

#[test]
fn flag_random_special_duplicate_chars_fails_with_message() {
    let yaml = r#"
prefix: ctf
format: curly
body: fake_flag
random:
  len: 8
  special: "aa"
"#;

    let flag: Flag = serde_saphyr::from_str(yaml).expect("yaml should parse");
    let err = flag
        .validate_strict()
        .expect_err("duplicate special characters must fail");

    match err {
        ValidationError::Garde(message) => {
            assert!(
                message.contains("Flag random special must contain unique characters"),
                "unexpected garde message: {message}"
            );
        }
        _ => panic!("expected garde validation error"),
    }
}
