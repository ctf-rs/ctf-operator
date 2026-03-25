---
name: build-and-test
description: "Build, test, and lint the ctf-operator Rust workspace — covers all crates, Helm charts, Docker images, CRD sync, and local kind clusters"
---

# Build and Test — ctf-operator

You are a build-and-test assistant for the **ctf-operator** project, a Kubernetes-native CTF platform written in Rust. Your job is to help the user build, test, lint, and validate the project correctly.

## Workspace Overview

This is a Cargo workspace (resolver 2) with four active crates:

| Crate       | Package name   | Edition | Binary         | Purpose                                             |
| ----------- | -------------- | ------- | -------------- | --------------------------------------------------- |
| `operator/` | `ctf-operator` | 2021    | `ctf-operator` | Main Kubernetes operator (reconcilers for CTF CRDs) |
| `cli/`      | `ctf-cli`      | 2021    | `ctf`          | CLI for CRD generation, kind cluster bootstrap      |
| `app/`      | `app`          | 2024    | `app`          | Leptos web UI (SSR + WASM hydration)                |
| `bot/`      | `bot`          | 2024    | `bot`          | Discord bot (account linking, tickets)              |

There is also an `operator-old/` directory — this is archived and **not** a workspace member. Ignore it.

### Crate Dependencies

- `ctf-operator` depends on `ctf-cli` (shared CRD types)
- `app` (with `ssr` feature) depends on `ctf-cli`
- `ctf-cli` is the foundation crate for CRD type definitions and Kubernetes schemas

## Rust Toolchain

- **Minimum Rust version**: 1.93 (per Dockerfiles)
- The `app` crate requires the `wasm32-unknown-unknown` target for WASM compilation
- The `app` crate uses `cargo-leptos` for building (not plain `cargo build`)

## Build Commands

### Build the entire workspace (excluding app WASM)

```bash
cargo build --workspace
```

### Build individual crates

```bash
# Operator binary
cargo build -p ctf-operator --bin ctf-operator

# CLI binary
cargo build -p ctf-cli --bin ctf

# Bot binary
cargo build -p bot

# App (server-side only, no WASM)
cargo build -p app --features ssr
```

### Build the app with Leptos (includes WASM)

The `app` crate is a Leptos full-stack application. It requires `cargo-leptos` to build both the server binary and the WASM client bundle:

```bash
cargo leptos build
```

For a release build:

```bash
cargo leptos build --release
```

**Important**: Do NOT use `cargo build -p app` for a full app build — that will miss the WASM hydration bundle. Use `cargo leptos build` instead. Plain `cargo build -p app --features ssr` is acceptable for server-side-only compilation checks.

### Release builds

```bash
cargo build --release -p ctf-operator --bin ctf-operator
cargo build --release -p ctf-cli --bin ctf
cargo leptos build --release
```

## Test Commands

### Run all workspace tests

```bash
cargo test --workspace
```

### Run tests for individual crates

```bash
cargo test -p ctf-operator
cargo test -p ctf-cli
cargo test -p app
cargo test -p bot
```

### Integration tests

The operator has an integration test that validates CRD YAML files stay in sync between the generated `crd/` directory and the Helm chart `helm/ctf-operator/files/crds/`:

```bash
cargo test -p ctf-operator --test crd_sync
```

If this test fails, it means someone updated CRD types without regenerating the YAML. The fix is to regenerate CRDs using the CLI:

```bash
cargo run -p ctf-cli -- crd generate
```

Then copy the output to both `crd/` and `helm/ctf-operator/files/crds/`.

### Helm chart unit tests

Requires the `helm-unittest` plugin:

```bash
# Install plugin (one-time)
helm plugin install https://github.com/helm-unittest/helm-unittest

# Run operator chart tests
helm unittest helm/ctf-operator

# Run app chart tests
helm unittest helm/ctf-app
```

Helm test files live in:

- `helm/ctf-operator/tests/` — configmap, deployment, CRDs, watch-namespace validation
- `helm/ctf-app/tests/` — deployment, CTF resource, service, RBAC

## Linting

### Clippy

The workspace has a very strict Clippy configuration. All of these lint categories are set to `warn`: `cargo`, `complexity`, `correctness`, `nursery`, `pedantic`, `perf`, `style`, `suspicious`. Many `restriction` group lints are also enabled.

Run Clippy for the whole workspace:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

For individual crates:

```bash
cargo clippy -p ctf-operator --all-targets -- -D warnings
cargo clippy -p ctf-cli --all-targets -- -D warnings
cargo clippy -p bot --all-targets -- -D warnings
cargo clippy -p app --all-targets --features ssr -- -D warnings
```

### Key clippy.toml rules

- `allow-panic-in-tests = true` — `panic!` and `unwrap()` are allowed in test code
- `allow-unwrap-in-tests = true` — `.unwrap()` is allowed in test code
- `warn-on-all-wildcard-imports = true` — wildcard imports produce warnings
- `avoid-breaking-exported-api = false` — Clippy won't suppress suggestions that would break public API

### What this means for writing code

- In production code: use `anyhow::Result`, `thiserror`, or `?` instead of `.unwrap()` / `panic!()`
- In production code: avoid `#[allow(...)]` without a `reason = "..."` attribute
- In test code: `.unwrap()` and `panic!()` are fine
- Use explicit imports, not wildcard `use module::*`
- In production code: avoid `#[expect(..)]` it is preferred to refactor code if possible

### Format check

```bash
cargo fmt --all -- --check
```

## Docker Builds

### Operator image

```bash
docker build -f Dockerfile.operator -t ctf-operator:local .
```

Uses a multi-stage build: `rust:1.93` builder → `debian:trixie-slim` runtime.
Builds only `ctf-operator` and `ctf-cli` crate sources.

### App image

```bash
docker build -f Dockerfile.app -t ctf-app:local .
```

Uses `cargo-leptos` inside the build stage for WASM compilation.
Installs the `wasm32-unknown-unknown` target automatically.
Copies both the server binary and the `target/site` directory to the final image.

## Local Development Prerequisites — docker-compose

Before running the app or operator locally, start the required side services using docker-compose. This is **critical** — tests and local runs will fail without these services.

```bash
docker compose -f app/docker-compose.yml up -d
```

This starts:

| Service      | Image                            | Port(s)                                                  | Purpose                                                                                    |
| ------------ | -------------------------------- | -------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| **postgres** | `postgres:17`                    | `5432`                                                   | App database (`ctf_app`, user: `ctf`, password: `ctf`)                                     |
| **keycloak** | `quay.io/keycloak/keycloak:26.1` | `8081`                                                   | OIDC identity provider (admin/admin). Imports dev realm from `app/dev/keycloak-realm.json` |
| **garage**   | `dxflrs/garage:v1.2.0`           | `3900`, `3901`, `3903`                                   | S3-compatible object storage for challenge files. Config at `app/dev/garage.toml`          |
| **lgtm**     | `grafana/otel-lgtm:0.10.0`       | `3000` (Grafana), `4317` (OTLP gRPC), `4318` (OTLP HTTP) | Observability stack (Grafana + Loki + Tempo + Mimir + OTel collector)                      |

Wait for postgres to be healthy before starting the app:

```bash
docker compose -f app/docker-compose.yml up -d --wait
```

To stop all services:

```bash
docker compose -f app/docker-compose.yml down
```

To wipe all data and start fresh:

```bash
docker compose -f app/docker-compose.yml down -v
```

Dev configuration files live in `app/dev/`:

- `keycloak-realm.json` — pre-configured OIDC realm for local dev
- `garage.toml` — S3 storage configuration
- `grafana/` — dashboard and datasource provisioning

The `.env` file at the repo root ties these services together:

```
DATABASE_URL=postgres://ctf:ctf@localhost:5432/ctf_app
OIDC_ISSUER_URL=http://localhost:8081/realms/ctf
CHALLENGE_FILES_S3_ENDPOINT=http://localhost:3900
```

## Local Development with Kind

The CLI can bootstrap a full local environment:

```bash
# Basic cluster
cargo run -p ctf-cli -- cluster create

# With a CTF applied
cargo run -p ctf-cli -- cluster create --ctf ./examples/ctf.yaml

# With Cilium CNI
cargo run -p ctf-cli -- cluster create --cilium

# Without running the operator in-cluster (for local development)
cargo run -p ctf-cli -- cluster create --no-operator
```

The `--no-operator` flag is useful for developing: it installs CRDs and Helm chart but scales the operator to zero, so you can run the operator locally with:

```bash
cargo run -p ctf-operator -- operator
```

The kind cluster exposes ports 12000–12020 for CTF challenge instances (see `kind-config.yaml`).

### Environment variables for local runs

The `.env` file defines defaults for local development:

```
APP_BIND_ADDR=0.0.0.0:8090
DATABASE_URL=postgres://ctf:ctf@localhost:5432/ctf_app
WATCH_NAMESPACES=ctf
OIDC_ISSUER_URL=http://localhost:8081/realms/ctf
CHALLENGE_FILES_S3_ENDPOINT=http://localhost:3900
```

## CRD Management

CRDs are defined as Rust types in `ctf-cli` (using `kube::CustomResource` derive). Generated YAML lives in two places that must stay in sync:

1. `crd/` — canonical generated CRDs
2. `helm/ctf-operator/files/crds/` — copy used by Helm chart

The six CRD types are: `challenges`, `ctfs`, `flags`, `instances`, `portclaims`, `reservationsets`.

The `crd_sync` integration test enforces they match. If you change CRD types, regenerate and copy.

## Common Workflows

### "I changed operator reconciliation logic"

```bash
cargo clippy -p ctf-operator --all-targets -- -D warnings
cargo test -p ctf-operator
```

### "I changed CRD type definitions"

```bash
cargo run -p ctf-cli -- crd generate
# Copy generated YAML to both crd/ and helm/ctf-operator/files/crds/
cargo test -p ctf-operator --test crd_sync
helm unittest helm/ctf-operator
```

### "I changed the Helm chart"

```bash
helm unittest helm/ctf-operator
cargo test -p ctf-operator --test crd_sync
```

### "I changed the web app"

```bash
cargo clippy -p app --all-targets --features ssr -- -D warnings
cargo leptos build
```

### "I changed the Discord bot"

```bash
cargo clippy -p bot --all-targets -- -D warnings
cargo build -p bot
```

### "Full CI-style validation"

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
helm unittest helm/ctf-operator
helm unittest helm/ctf-app
```

### "Starting local development from scratch"

```bash
docker compose -f app/docker-compose.yml up -d --wait
cargo run -p ctf-cli -- cluster create --no-operator --ctf ./examples/ctf.yaml
cargo run -p ctf-operator -- operator
# In another terminal:
cargo leptos watch
```

## Troubleshooting

| Problem                                              | Cause                                  | Fix                                                                                    |
| ---------------------------------------------------- | -------------------------------------- | -------------------------------------------------------------------------------------- |
| `crd_sync` test fails                                | CRD YAML out of sync                   | Regenerate CRDs with `cargo run -p ctf-cli -- crd generate` and copy to both locations |
| `cargo build -p app` fails with missing features     | App needs feature flags                | Use `--features ssr` for server or `cargo leptos build` for full build                 |
| WASM target missing                                  | `wasm32-unknown-unknown` not installed | `rustup target add wasm32-unknown-unknown`                                             |
| `cargo-leptos` not found                             | Tool not installed                     | `cargo install cargo-leptos`                                                           |
| Clippy errors about `unwrap_used` in production code | Strict linting                         | Use `?` operator or `anyhow` error handling instead of `.unwrap()`                     |
| `helm unittest` not found                            | Plugin not installed                   | `helm plugin install https://github.com/helm-unittest/helm-unittest`                   |
| App fails to connect to postgres/keycloak/S3         | Side services not running              | `docker compose -f app/docker-compose.yml up -d --wait`                                |
| Postgres connection refused on 5432                  | Container not healthy yet              | Wait or check `docker compose -f app/docker-compose.yml ps`                            |
