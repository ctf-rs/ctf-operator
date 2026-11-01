---
name: build-and-test
description: "Build, test, and lint the ctf-operator Rust workspace, operator Helm chart, Docker image, and CRD sync"
---

# Build and Test - ctf-operator

## Workspace

This Cargo workspace contains two crates:

| Directory | Package | Binary | Purpose |
| --- | --- | --- | --- |
| `operator/` | `ctf-operator` | `ctf-operator` | Kubernetes operator and CRD generation |
| `cli/` | `ctf-cli` | `ctf` | Challenge image builds, file uploads, and kind cluster bootstrap |

The operator depends on `ctf-cli` for shared domain models and CRD types.
The operator Dockerfile uses Rust 1.95.

## Build

```bash
cargo build --workspace
cargo build --release -p ctf-operator --bin ctf-operator
cargo build --release -p ctf-cli --bin ctf
```

## Tests

```bash
cargo test --workspace
```

The operator integration test checks that the generated CRDs and the copies
packaged in the Helm chart match:

```bash
cargo test -p ctf-operator --test crd_sync
```

## Formatting and linting

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

The workspace enables strict Rust and Clippy lints. Production code should
propagate errors rather than use `unwrap()` or `panic!()`. Use explicit imports.
Avoid suppressing lints; any necessary suppression must explain its reason.
`clippy.toml` permits panics and unwraps in tests.

## CRD generation

The six CRD types are `challenges`, `ctfs`, `flags`, `instances`, `portclaims`,
and `reservationsets`. Their Rust definitions live in `cli/src/k8s/models/`.

Generate their YAML using the operator binary:

```bash
cargo run -p ctf-operator -- crd
```

This command replaces the generated `crd/` directory. Copy the generated files
into `helm/ctf-operator/files/crds/`:

```bash
cp crd/*.yaml helm/ctf-operator/files/crds/
cargo test -p ctf-operator --test crd_sync
```

Keep one Kubernetes resource per YAML file.

## Helm chart

```bash
helm lint helm/ctf-operator
helm unittest helm/ctf-operator
```

If the unittest plugin is missing:

```bash
helm plugin install https://github.com/helm-unittest/helm-unittest
```

Tests live in `helm/ctf-operator/tests/`.

## Docker image

```bash
docker build -f Dockerfile.operator -t ctf-operator:local .
```

The multi-stage build copies only the CLI and operator manifests and sources.
The final image contains the operator binary and CA certificates.

## Local development

Cluster bootstrap requires `kind`, `kubectl`, `helm`, and a supported container
provider (Docker, Podman, or nerdctl). Cilium is optional and requires its CLI.
The CTF YAML is a required positional argument:

```bash
cargo run -p ctf-cli -- cluster create ./examples/ctf.yaml
```

The cluster name and exposed port range come from the CTF configuration.
The CLI pulls the operator chart from its configured OCI registry and applies
the CTF and referenced challenges. It does not build a local operator image.
The CLI deletes an existing kind cluster with the same name before recreating
it; only run this command when the user has authorized that operation.

For a manual local operator run, bootstrap with the in-cluster operator disabled:

```bash
cargo run -p ctf-cli -- cluster create ./examples/ctf.yaml --no-operator
cargo run -p ctf-operator -- operator \
  --namespace ctf \
  --instance-namespace-prefix ctf-inst- \
  --config ./examples/operator-config-local.yaml
```

`scripts/dev/run-operator.sh` is an alternative launcher targeting the
`kind-ctf-dev` context. `kind-config.yaml` is a standalone local cluster
configuration; the CLI generates its own kind configuration.

The operator does not require a database or identity provider. CLI file upload
requires S3 storage configured in the CTF YAML and explicitly supplied credentials
or supported AWS/S3 environment variables.

Do not modify live Kubernetes resources without explicit user authorization.

## Validation by change

| Change | Checks |
| --- | --- |
| Operator or CLI Rust code | Formatting, Clippy, and affected crate tests |
| Shared CRD models | Rust checks, regenerate both YAML copies, CRD sync test, Helm tests |
| Helm templates | Helm lint and unit tests |
| Workspace or Docker build configuration | Workspace Rust checks and Docker build when available |

Full workspace validation:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
helm lint helm/ctf-operator
helm unittest helm/ctf-operator
```
