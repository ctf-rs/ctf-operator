# ctf-operator

## Helm chart

Install the operator chart:

```bash
helm upgrade --install ctf-operator ./helm/ctf-operator \
	--namespace ctf-operator \
	--create-namespace \
	--set watchNamespaces[0]=ctf \
	--set instanceNamespacePrefix=ctf-inst-
```

Install with additional watched namespaces:

```bash
helm upgrade --install ctf-operator ./helm/ctf-operator \
	--namespace ctf-operator \
	--create-namespace \
	--set watchNamespaces[0]=ctf \
	--set watchNamespaces[1]=ctf-admin \
	--set instanceNamespacePrefix=ctf-inst-
```

Disable CRD install from this chart release:

```bash
helm upgrade --install ctf-operator ./helm/ctf-operator \
	--namespace ctf-operator \
	--create-namespace \
	--set crds.install=false
```

Default security hardening in chart:

- `runAsNonRoot` with non-root UID/GID
- `seccompProfile: RuntimeDefault`
- `allowPrivilegeEscalation: false`
- dropped Linux capabilities
- `readOnlyRootFilesystem: true`

The service account is cluster-scoped via `ClusterRole` + `ClusterRoleBinding` to allow dynamic namespace creation and instance provisioning in new namespaces.

Namespace safety is enforced in operator logic using:

- `watchNamespaces`: explicit namespaces
- `instanceNamespacePrefix`: prefix used for per-instance runtime namespaces created by operator

Resources in namespaces that are not `watchNamespaces` (for CRD control plane) and not `instanceNamespacePrefix*` (for provisioned runtime resources) are ignored by reconcilers.

Permissions include:

- operator CRDs (`ctfs`, `challenges`, `flags`, `instances`, `portclaims`, `reservationsets`)
- status/finalizer updates
- creating/updating/deleting `Deployments` and `Services`
- namespace creation for dynamically provisioned instance namespaces

## CLI cluster create

The CLI now includes a local cluster bootstrap flow using `kind`.

```bash
cargo run -p ctf -- cluster create
```

What it does:

1. creates a local `kind` cluster (default name: `ctf`)
2. installs Cilium (`cilium install --wait`)
3. builds `ctf-operator:local` image and loads it into kind
4. installs the operator from `./helm/ctf-operator`
5. optionally applies a CTF and all referenced challenges

Example with CTF apply:

```bash
cargo run -p ctf -- cluster create --ctf ./examples/ctf.yaml
```

Example with multiple watched namespaces:

```bash
cargo run -p ctf -- cluster create \
	--namespace ctf,ctf-admin \
	--instance-namespace-prefix ctf-inst- \
	--operator-replicas 1
```

`operator-replicas` defaults to `1` because leader election/lease coordination is not yet implemented.

## Helm unit tests

Install the Helm unittest plugin:

```bash
helm plugin install https://github.com/helm-unittest/helm-unittest
```

Run chart unit tests:

```bash
helm unittest helm/ctf-operator
```
