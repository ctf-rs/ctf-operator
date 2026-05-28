#!/usr/bin/env bash
# Run the CTF app locally (Leptos SSR) against kind + docker-compose services.
#
# Usage: ./scripts/dev/run-app.sh

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

CLUSTER_NAME="ctf-dev"

# Ensure we're pointed at the right cluster
kubectl config use-context "kind-${CLUSTER_NAME}" 2>/dev/null || true

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

echo "=== Building app ==="
cargo build -p app --features ssr

echo ""
echo "=== Starting app on http://localhost:8090 ==="
exec cargo run -p app --features ssr
