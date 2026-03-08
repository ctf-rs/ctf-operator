use std::{fs, path::Path};

use serde_json::Value;

#[test]
fn helm_crds_match_generated_crds() {
    let generated_dir = Path::new("..").join("crd");
    let helm_dir = Path::new("..")
        .join("helm")
        .join("ctf-operator")
        .join("files")
        .join("crds");

    let crd_files = [
        "challenges.yaml",
        "ctfs.yaml",
        "flags.yaml",
        "instances.yaml",
        "portclaims.yaml",
        "reservationsets.yaml",
    ];

    for file in crd_files {
        let generated_path = generated_dir.join(file);
        let helm_path = helm_dir.join(file);

        assert!(
            generated_path.exists(),
            "generated CRD missing: {}",
            generated_path.display()
        );
        assert!(
            helm_path.exists(),
            "helm CRD missing: {}",
            helm_path.display()
        );

        let generated_docs = parse_yaml_docs(&generated_path);
        let helm_docs = parse_yaml_docs(&helm_path);

        assert_eq!(
            generated_docs,
            helm_docs,
            "CRD mismatch for file {}\n  generated: {}\n  helm: {}",
            file,
            generated_path.display(),
            helm_path.display()
        );
    }
}

fn parse_yaml_docs(path: &Path) -> Vec<Value> {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed reading {}: {err}", path.display()));

    let value = serde_saphyr::from_str::<Value>(&content)
        .unwrap_or_else(|err| panic!("failed parsing YAML in {}: {err}", path.display()));

    if matches!(value, Value::Null) {
        Vec::new()
    } else {
        vec![value]
    }
}
