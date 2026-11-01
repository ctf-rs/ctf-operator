use std::{fs, path::Path};

use ctf_cli::k8s::{Challenge, Ctf, Flag, Instance, PortClaim, ReservationSet};
use kube::CustomResourceExt;
use serde_json::Value;

#[test]
fn helm_crds_match_generated_crds() {
    let helm_dir = Path::new("..")
        .join("helm")
        .join("ctf-operator")
        .join("files")
        .join("crds");

    let crds = [
        ("challenges.yaml", Challenge::crd()),
        ("ctfs.yaml", Ctf::crd()),
        ("flags.yaml", Flag::crd()),
        ("instances.yaml", Instance::crd()),
        ("portclaims.yaml", PortClaim::crd()),
        ("reservationsets.yaml", ReservationSet::crd()),
    ];

    for (file, generated) in crds {
        let helm_path = helm_dir.join(file);

        assert!(
            helm_path.exists(),
            "helm CRD missing: {}",
            helm_path.display()
        );

        let generated_doc = serde_json::to_value(generated)
            .unwrap_or_else(|err| unreachable!("failed serializing CRD for {file}: {err}"));
        let helm_doc = parse_yaml_doc(&helm_path);

        assert_eq!(
            generated_doc,
            helm_doc,
            "CRD mismatch for file {}\n  helm: {}",
            file,
            helm_path.display()
        );
    }
}

fn parse_yaml_doc(path: &Path) -> Value {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|err| unreachable!("failed reading {}: {err}", path.display()));

    serde_saphyr::from_str::<Value>(&content)
        .unwrap_or_else(|err| unreachable!("failed parsing YAML in {}: {err}", path.display()))
}
