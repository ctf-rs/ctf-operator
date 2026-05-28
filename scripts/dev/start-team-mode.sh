#!/usr/bin/env bash
# Start a COMPLETE clean environment in TEAM MODE.
#
# This script:
#   1. Tears down any existing environment
#   2. Brings up docker-compose (Postgres, Keycloak, Garage, LGTM)
#   3. Creates a kind cluster with CRDs
#   4. Configures Keycloak with team attributes + protocol mappers
#   5. Applies CTF + challenge resources
#   6. Builds and starts the operator (background)
#   7. Builds and starts the app (foreground)
#
# Usage: ./scripts/dev/start-team-mode.sh
#
# Test accounts (password = username):
#   Team Alpha: player, test2, test3
#   Team Beta:  test4, test5, test6
#   Solo users: test7, test8, test9, test10
#   Admin:      test1 (no team)

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

CLUSTER_NAME="ctf-dev"
CTF_NAMESPACE="ctf"
SCRIPT_DIR="$(cd scripts/dev && pwd)"

# ── Cleanup ──────────────────────────────────────────────────────────
echo "=== Tearing down existing environment ==="
"${SCRIPT_DIR}/env-down.sh" --full 2>/dev/null || true

# ── Infrastructure ───────────────────────────────────────────────────
echo ""
echo "=== Starting docker-compose services ==="
(cd app && docker compose up -d)

echo ""
echo "=== Waiting for Postgres ==="
for i in $(seq 1 30); do
    if PGPASSWORD=ctf psql -h localhost -U ctf -d ctf_app -c "SELECT 1" > /dev/null 2>&1; then
        echo "  Postgres ready."
        break
    fi
    [ "$i" -eq 30 ] && { echo "  ERROR: Postgres timeout"; exit 1; }
    sleep 1
done

echo ""
echo "=== Waiting for Keycloak ==="
for i in $(seq 1 90); do
    if curl -sf http://localhost:8081/realms/ctf > /dev/null 2>&1; then
        echo "  Keycloak ready."
        break
    fi
    [ "$i" -eq 90 ] && { echo "  ERROR: Keycloak timeout"; exit 1; }
    sleep 2
done

# ── Kind cluster ─────────────────────────────────────────────────────
echo ""
echo "=== Creating kind cluster ==="
kind create cluster --name "${CLUSTER_NAME}" --config kind-config.yaml
kubectl config use-context "kind-${CLUSTER_NAME}"

echo ""
echo "=== Installing CRDs ==="
kubectl apply -f crd/

echo ""
echo "=== Creating namespace and CTF resources ==="
kubectl create namespace "${CTF_NAMESPACE}" --dry-run=client -o yaml | kubectl apply -f -
kubectl apply -n "${CTF_NAMESPACE}" -f examples/ctf-crd.yaml

# Apply ALL challenge YAML files (excluding non-challenge configs)
CHALLENGE_FILES=()
for f in examples/misc-*.yaml examples/crypto-*.yaml examples/web-*.yaml \
         examples/foren-*.yaml examples/rev-*.yaml examples/pwn-*.yaml; do
    [ -f "$f" ] && CHALLENGE_FILES+=("$f")
done
CHALLENGE_NAMES=()
for f in "${CHALLENGE_FILES[@]}"; do
    if kubectl apply -n "${CTF_NAMESPACE}" -f "$f" 2>/dev/null; then
        name=$(basename "$f" .yaml)
        CHALLENGE_NAMES+=("$name")
        echo "  Applied ${name}"
    fi
done

# Update CTF to reference all applied challenges
CHALLENGES_YAML=""
for name in "${CHALLENGE_NAMES[@]}"; do
    CHALLENGES_YAML="${CHALLENGES_YAML}    - ${name}\n"
done

cat <<EOF | kubectl apply -n "${CTF_NAMESPACE}" -f -
apiVersion: operator.ctf.rs/v1alpha1
kind: Ctf
metadata:
  name: playground
spec:
  fullName: Playground
  public: true
  flag:
    prefix: SCC
    format: curly
    random:
      len: 8
    options:
      ignoreRandom: false
  network:
    ports:
      start: 12000
      end: 12020
  challenges:
$(echo -e "${CHALLENGES_YAML}")EOF

# ── Keycloak team setup ──────────────────────────────────────────────
echo ""
echo "=== Configuring Keycloak for team mode ==="
"${SCRIPT_DIR}/setup-teams.sh"

# ── Build ────────────────────────────────────────────────────────────
echo ""
echo "=== Building operator ==="
cargo build -p ctf-operator 2>&1 | tail -3

echo ""
echo "=== Building app (SSR + WASM hydration) ==="
cargo leptos build 2>&1 | tail -5

# ── Start operator in background ─────────────────────────────────────
echo ""
echo "=== Starting operator (background) ==="
RUST_LOG=info cargo run -p ctf-operator -- operator \
    --namespace "${CTF_NAMESPACE}" \
    --config examples/operator-config-local.yaml \
    > /tmp/ctf-operator.log 2>&1 &
OPERATOR_PID=$!
echo "  Operator PID: ${OPERATOR_PID} (log: /tmp/ctf-operator.log)"

# Wait for operator to initialise
sleep 3
if ! kill -0 "${OPERATOR_PID}" 2>/dev/null; then
    echo "  ERROR: Operator exited. Last log lines:"
    tail -10 /tmp/ctf-operator.log
    exit 1
fi

# ── Trap to clean up operator on exit ────────────────────────────────
cleanup() {
    echo ""
    echo "=== Stopping operator (PID ${OPERATOR_PID}) ==="
    kill "${OPERATOR_PID}" 2>/dev/null || true
    wait "${OPERATOR_PID}" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# ── Seed submissions for scoreboard testing ──────────────────────────
echo ""
echo "=== Seeding scoreboard submissions ==="
"${SCRIPT_DIR}/seed-submissions.sh"

# ── Start app in foreground ──────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║               CTF Platform — TEAM MODE                     ║"
echo "╠══════════════════════════════════════════════════════════════╣"
echo "║  App:       http://localhost:8090                           ║"
echo "║  Keycloak:  http://localhost:8081  (admin/admin)            ║"
echo "║  Grafana:   http://localhost:3000  (admin/admin)            ║"
echo "║                                                             ║"
echo "║  Team Alpha:  player, test2, test3                          ║"
echo "║  Team Beta:   test4, test5, test6                           ║"
echo "║  Solo users:  test7, test8, test9, test10                   ║"
echo "║  Admin:       test1                                         ║"
echo "║  (password = username for all accounts)                     ║"
echo "║                                                             ║"
echo "║  Press Ctrl+C to stop                                       ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

export DATABASE_URL="postgres://ctf:ctf@localhost:5432/ctf_app"
export OIDC_ISSUER_URL="http://localhost:8081/realms/ctf"
export OIDC_PUBLIC_ISSUER_URL="http://localhost:8081/realms/ctf"
export OIDC_CLIENT_ID="ctf-app"
export OIDC_CLIENT_SECRET="ctf-app-secret"
export OIDC_REDIRECT_URL="http://localhost:8090/oauth/callback"
export CTF_NAME="playground"
export CTF_NAMESPACE="ctf"
export APP_BIND_ADDR="0.0.0.0:8090"
export INSTANCE_NAME_PREFIX="ctf-inst"
export OIDC_ADMIN_ROLE="ctf-admin"
export OIDC_PLAYER_ROLE="ctf-player"
export RUST_LOG="info,app=debug"
export LEPTOS_SITE_ROOT="target/site"
export LEPTOS_SITE_PKG_DIR="pkg"

exec cargo leptos serve
