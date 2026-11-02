use std::{fs, path::Path};

use ctf_core::k8s::{Challenge, Ctf, Flag, Instance, PortClaim, ReservationSet};
use kube::CustomResourceExt;
use serde_json::Value;

#[test]
fn checked_in_crds_match_generated_crds() {
    let crd_dir = Path::new("..").join("crd");

    let crds = [
        ("challenges.yaml", Challenge::crd()),
        ("ctfs.yaml", Ctf::crd()),
        ("flags.yaml", Flag::crd()),
        ("instances.yaml", Instance::crd()),
        ("portclaims.yaml", PortClaim::crd()),
        ("reservationsets.yaml", ReservationSet::crd()),
    ];

    for (file, generated) in crds {
        let crd_path = crd_dir.join(file);

        assert!(crd_path.exists(), "CRD missing: {}", crd_path.display());

        let generated_doc = serde_json::to_value(generated)
            .unwrap_or_else(|err| unreachable!("failed serializing CRD for {file}: {err}"));
        let stored_doc = parse_yaml_doc(&crd_path);

        assert_eq!(
            generated_doc,
            stored_doc,
            "CRD mismatch for file {}\n  stored: {}",
            file,
            crd_path.display()
        );
    }
}

fn parse_yaml_doc(path: &Path) -> Value {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|err| unreachable!("failed reading {}: {err}", path.display()));

    serde_saphyr::from_str::<Value>(&content)
        .unwrap_or_else(|err| unreachable!("failed parsing YAML in {}: {err}", path.display()))
}
