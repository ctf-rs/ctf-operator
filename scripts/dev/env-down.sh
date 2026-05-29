#!/usr/bin/env bash
# Tear down the local dev environment.
#
# Usage: ./scripts/dev/env-down.sh [--full]
#   --full  Also delete the kind cluster and docker volumes

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

CLUSTER_NAME="ctf-dev"
FULL=false
if [[ "${1:-}" == "--full" ]]; then
    FULL=true
fi

echo "=== Stopping docker-compose services ==="
(cd app && docker compose down ${FULL:+--volumes})

if $FULL; then
    echo ""
    echo "=== Deleting kind cluster '${CLUSTER_NAME}' ==="
    kind delete cluster --name "${CLUSTER_NAME}" 2>/dev/null || true
fi

echo ""
echo "=== Done ==="
