#!/usr/bin/env bash
# Quick status check for the dev environment.
#
# Usage: ./scripts/dev/status.sh

set -euo pipefail

echo "=== Docker Compose Services ==="
(cd "$(git rev-parse --show-toplevel)/app" && docker compose ps 2>/dev/null) || echo "  Not running"

echo ""
echo "=== Kind Cluster ==="
if kind get clusters 2>/dev/null | grep -q "^ctf-dev$"; then
    echo "  Cluster 'ctf-dev' exists"
    kubectl --context kind-ctf-dev get nodes 2>/dev/null || echo "  Cannot connect"
else
    echo "  No 'ctf-dev' cluster"
fi

echo ""
echo "=== Kubernetes Resources ==="
kubectl --context kind-ctf-dev get ctf,challenges,instances -n ctf 2>/dev/null || echo "  Not available"

echo ""
echo "=== Database ==="
docker exec app-postgres-1 psql -U ctf -d ctf_app -t -c \
    "SELECT 'users: ' || count(*) FROM users UNION ALL SELECT 'sessions: ' || count(*) FROM sessions UNION ALL SELECT 'submissions: ' || count(*) FROM flag_submissions UNION ALL SELECT 'solved: ' || count(*) FROM solved_challenges;" 2>/dev/null || echo "  Not available"

echo ""
echo "=== Endpoints ==="
curl -sf -o /dev/null -w "  App (8090):      %{http_code}\n" http://localhost:8090 2>/dev/null || echo "  App (8090):      DOWN"
curl -sf -o /dev/null -w "  Keycloak (8081): %{http_code}\n" http://localhost:8081/realms/ctf 2>/dev/null || echo "  Keycloak (8081): DOWN"
curl -sf -o /dev/null -w "  Grafana (3000):  %{http_code}\n" http://localhost:3000 2>/dev/null || echo "  Grafana (3000):  DOWN"
