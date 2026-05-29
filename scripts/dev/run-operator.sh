#!/usr/bin/env bash
# Run the CTF operator locally against the kind cluster.
#
# Usage: ./scripts/dev/run-operator.sh

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

CLUSTER_NAME="ctf-dev"
CTF_NAMESPACE="ctf"

# Ensure we're pointed at the right cluster
kubectl config use-context "kind-${CLUSTER_NAME}" 2>/dev/null || true

echo "=== Building operator ==="
cargo build -p ctf-operator

echo ""
echo "=== Starting operator ==="
exec cargo run -p ctf-operator -- operator \
    --namespaces "${CTF_NAMESPACE}" \
    --instance-namespace-prefix "ctf-inst-" \
    --config examples/operator-config-local.yaml
