#!/usr/bin/env bash
# Bring up the full local dev environment:
#   1. docker-compose (Postgres, Keycloak, Garage, LGTM)
#   2. kind cluster with CRDs installed
#
# Usage: ./scripts/dev/env-up.sh

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

CLUSTER_NAME="ctf-dev"
CTF_NAMESPACE="ctf"

echo "=== Step 1: Starting docker-compose services ==="
(cd app && docker compose up -d)

echo ""
echo "=== Step 2: Waiting for Postgres to be ready ==="
for i in $(seq 1 30); do
    if docker exec app-postgres-1 pg_isready -U ctf -d ctf_app -q 2>/dev/null; then
        echo "  Postgres is ready."
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "  ERROR: Postgres did not become ready in time."
        exit 1
    fi
    sleep 1
done

echo ""
echo "=== Step 3: Waiting for Keycloak to be ready ==="
for i in $(seq 1 60); do
    if curl -sf http://localhost:8081/realms/ctf > /dev/null 2>&1; then
        echo "  Keycloak is ready."
        break
    fi
    if [ "$i" -eq 60 ]; then
        echo "  ERROR: Keycloak did not become ready in time."
        exit 1
    fi
    sleep 2
done

echo ""
echo "=== Step 4: Creating kind cluster '${CLUSTER_NAME}' ==="
if kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
    echo "  Cluster '${CLUSTER_NAME}' already exists, skipping creation."
else
    kind create cluster --name "${CLUSTER_NAME}" --config kind-config.yaml
fi

echo ""
echo "=== Step 5: Installing CRDs ==="
kubectl apply -f crd/

echo ""
echo "=== Step 6: Creating CTF namespace and resources ==="
kubectl create namespace "${CTF_NAMESPACE}" --dry-run=client -o yaml | kubectl apply -f -
kubectl apply -n "${CTF_NAMESPACE}" -f examples/ctf.yaml
kubectl apply -n "${CTF_NAMESPACE}" -f examples/ctf-crd.yaml

echo ""
echo "=== Step 7: Loading challenge-test image into kind ==="
if docker image inspect challenge-test:latest >/dev/null 2>&1; then
    echo "  Image already exists, loading into kind..."
else
    echo "  Building challenge-test image..."
    docker build -t challenge-test:latest examples/challenge-test/
fi
kind load docker-image challenge-test:latest --name "${CLUSTER_NAME}" 2>/dev/null || true

echo ""
echo "=== Environment is ready! ==="
echo ""
echo "Services:"
echo "  Postgres:  localhost:5432 (ctf/ctf/ctf_app)"
echo "  Keycloak:  http://localhost:8081 (admin/admin)"
echo "  Garage:    localhost:3900 (S3 API)"
echo "  Grafana:   http://localhost:3000 (admin/admin)"
echo ""
echo "Kind cluster: ${CLUSTER_NAME}"
echo "  kubectl config use-context kind-${CLUSTER_NAME}"
echo ""
echo "Next steps:"
echo "  1. Run the operator:  ./scripts/dev/run-operator.sh"
echo "  2. Run the app:       ./scripts/dev/run-app.sh"
echo "  3. Open browser:      http://localhost:8090"
