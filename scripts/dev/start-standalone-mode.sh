#!/usr/bin/env bash
# Start a COMPLETE clean environment in STANDALONE (solo) MODE.
#
# This script:
#   1. Tears down any existing environment
#   2. Brings up docker-compose (Postgres, Keycloak, Garage, LGTM)
#   3. Creates a kind cluster with CRDs
#   4. Applies CTF + challenge resources
#   5. Builds and starts the operator (background)
#   6. Builds and starts the app (foreground)
#
# No team attributes are configured — all users operate in solo mode.
#
# Usage: ./scripts/dev/start-standalone-mode.sh
#
# Test accounts (password = username):
#   Players: player, test2–test10
#   Admin:   test1

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

# Apply challenges that pass validation
for f in examples/misc-sanity.yaml examples/misc-trivia.yaml examples/web-hello.yaml; do
    kubectl apply -n "${CTF_NAMESPACE}" -f "$f" 2>/dev/null && \
        echo "  Applied $(basename "$f")" || true
done

# Update CTF to reference only the challenges we applied
cat <<'EOF' | kubectl apply -n "${CTF_NAMESPACE}" -f -
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
    - misc-sanity
    - misc-trivia
    - web-hello
EOF

# ── Enable direct access grants for testing ──────────────────────────
echo ""
echo "=== Enabling Keycloak direct access grants ==="
KC_TOKEN=$(curl -sf -X POST "http://localhost:8081/realms/master/protocol/openid-connect/token" \
    -d "client_id=admin-cli&username=admin&password=admin&grant_type=password" \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['access_token'])")

CLIENT_UUID=$(curl -sf "http://localhost:8081/admin/realms/ctf/clients?clientId=ctf-app" \
    -H "Authorization: Bearer ${KC_TOKEN}" \
    | python3 -c "import sys,json; print(json.load(sys.stdin)[0]['id'])")

CLIENT_JSON=$(curl -sf "http://localhost:8081/admin/realms/ctf/clients/${CLIENT_UUID}" \
    -H "Authorization: Bearer ${KC_TOKEN}")
UPDATED_CLIENT=$(echo "${CLIENT_JSON}" | python3 -c "
import sys, json
c = json.load(sys.stdin)
c['directAccessGrantsEnabled'] = True
print(json.dumps(c))
")
curl -sf -o /dev/null -X PUT "http://localhost:8081/admin/realms/ctf/clients/${CLIENT_UUID}" \
    -H "Authorization: Bearer ${KC_TOKEN}" \
    -H "Content-Type: application/json" \
    -d "${UPDATED_CLIENT}"

# Remove required constraint from email/firstName/lastName for test users
PROFILE=$(curl -sf "http://localhost:8081/admin/realms/ctf/users/profile" \
    -H "Authorization: Bearer ${KC_TOKEN}")
UPDATED_PROFILE=$(echo "${PROFILE}" | python3 -c "
import sys, json
profile = json.load(sys.stdin)
for attr in profile['attributes']:
    if attr['name'] in ('email', 'firstName', 'lastName'):
        attr.pop('required', None)
print(json.dumps(profile))
")
curl -sf -o /dev/null -X PUT "http://localhost:8081/admin/realms/ctf/users/profile" \
    -H "Authorization: Bearer ${KC_TOKEN}" \
    -H "Content-Type: application/json" \
    -d "${UPDATED_PROFILE}"

echo "  Done."

# ── Build ────────────────────────────────────────────────────────────
echo ""
echo "=== Building operator and app ==="
cargo build -p ctf-operator -p app --features ssr 2>&1 | tail -3

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

# ── Start app in foreground ──────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║             CTF Platform — STANDALONE MODE                  ║"
echo "╠══════════════════════════════════════════════════════════════╣"
echo "║  App:       http://localhost:8090                           ║"
echo "║  Keycloak:  http://localhost:8081  (admin/admin)            ║"
echo "║  Grafana:   http://localhost:3000  (admin/admin)            ║"
echo "║                                                             ║"
echo "║  Players:   player, test2–test10                            ║"
echo "║  Admin:     test1                                           ║"
echo "║  (password = username for all accounts)                     ║"
echo "║                                                             ║"
echo "║  No teams configured — all users are solo.                  ║"
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

exec cargo run -p app --features ssr
