#!/usr/bin/env bash
# Configure Keycloak for team mode testing.
# Creates team groups, a group-membership protocol mapper, and assigns test users
# to team groups. The app reads team info from the `groups` JWT claim.
#
# Usage: ./scripts/dev/setup-teams.sh
#
# Prerequisites: Keycloak running at localhost:8081 with the 'ctf' realm imported.

set -euo pipefail

KC_URL="http://localhost:8081"
KC_REALM="ctf"
KC_ADMIN_USER="admin"
KC_ADMIN_PASS="admin"

echo "=== Obtaining admin token ==="
TOKEN=$(curl -sf -X POST "${KC_URL}/realms/master/protocol/openid-connect/token" \
    -d "client_id=admin-cli" \
    -d "username=${KC_ADMIN_USER}" \
    -d "password=${KC_ADMIN_PASS}" \
    -d "grant_type=password" | python3 -c "import sys,json; print(json.load(sys.stdin)['access_token'])")

AUTH="Authorization: Bearer ${TOKEN}"

# Helper: GET with auth
kc_get() { curl -sf -H "${AUTH}" "${KC_URL}/admin/realms/${KC_REALM}$1"; }
# Helper: POST with auth (returns Location header for created resources)
kc_post() { curl -sf -X POST -H "${AUTH}" -H "Content-Type: application/json" -d "$2" "${KC_URL}/admin/realms/${KC_REALM}$1"; }
kc_post_loc() { curl -sf -o /dev/null -D - -X POST -H "${AUTH}" -H "Content-Type: application/json" -d "$2" "${KC_URL}/admin/realms/${KC_REALM}$1" | grep -i '^location:' | tr -d '\r' | awk '{print $2}'; }
# Helper: PUT with auth
kc_put() { curl -sf -o /dev/null -X PUT -H "${AUTH}" -H "Content-Type: application/json" -d "$2" "${KC_URL}/admin/realms/${KC_REALM}$1"; }

echo ""
echo "=== Setting up group-membership protocol mapper ==="

# Get client internal ID
CLIENT_ID=$(kc_get "/clients" | python3 -c "
import sys, json
for c in json.load(sys.stdin):
    if c['clientId'] == 'ctf-app':
        print(c['id'])
        break
")
echo "  ctf-app client id: ${CLIENT_ID}"

# Create group membership mapper (adds groups claim to JWT with full paths)
MAPPER_EXISTS=$(kc_get "/clients/${CLIENT_ID}/protocol-mappers/models" | python3 -c "
import sys, json
for m in json.load(sys.stdin):
    if m.get('name') == 'groups':
        print('yes')
        break
" 2>/dev/null || true)

if [ "${MAPPER_EXISTS}" != "yes" ]; then
    kc_post "/clients/${CLIENT_ID}/protocol-mappers/models" '{
        "name": "groups",
        "protocol": "openid-connect",
        "protocolMapper": "oidc-group-membership-mapper",
        "config": {
            "full.path": "true",
            "introspection.token.claim": "true",
            "id.token.claim": "true",
            "access.token.claim": "true",
            "claim.name": "groups",
            "userinfo.token.claim": "true"
        }
    }'
    echo "  Created groups mapper"
else
    echo "  groups mapper already exists"
fi

echo ""
echo "=== Enabling direct access grants (for testing) ==="
CLIENT_JSON=$(kc_get "/clients/${CLIENT_ID}")
UPDATED_CLIENT=$(echo "${CLIENT_JSON}" | python3 -c "
import sys, json
c = json.load(sys.stdin)
c['directAccessGrantsEnabled'] = True
print(json.dumps(c))
")
kc_put "/clients/${CLIENT_ID}" "${UPDATED_CLIENT}"
echo "  Direct access grants enabled for ctf-app"

echo ""
echo "=== Creating team groups ==="

# Create parent group "ctf-teams"
PARENT_LOC=$(kc_post_loc "/groups" '{"name": "ctf-teams"}')
PARENT_ID=$(echo "${PARENT_LOC}" | grep -oE '[^/]+$')
echo "  Created parent group ctf-teams (${PARENT_ID})"

# Create child groups with team_id attribute
create_team_group() {
    local name="$1" team_id="$2"
    local loc
    loc=$(kc_post_loc "/groups/${PARENT_ID}/children" "{
        \"name\": \"${name}\",
        \"attributes\": {
            \"team_id\": [\"${team_id}\"]
        }
    }")
    local gid
    gid=$(echo "${loc}" | grep -oE '[^/]+$')
    echo "  Created group ${name} → team_id=${team_id} (${gid})"
    echo "${gid}"
}

ALPHA_ID=$(create_team_group "team-alpha" "team-alpha")
BETA_ID=$(create_team_group "team-beta" "team-beta")

echo ""
echo "=== Assigning users to team groups ==="

# Helper: get user ID by username
get_user_id() {
    kc_get "/users?username=$1&exact=true" | python3 -c "
import sys, json
users = json.load(sys.stdin)
if users: print(users[0]['id'])
"
}

# Helper: add user to group
add_user_to_group() {
    local username="$1"
    local group_id="$2"
    local group_name="$3"

    local user_id
    user_id=$(get_user_id "${username}")
    if [ -z "${user_id}" ]; then
        echo "  WARNING: user '${username}' not found"
        return
    fi

    curl -sf -o /dev/null -X PUT -H "${AUTH}" \
        "${KC_URL}/admin/realms/${KC_REALM}/users/${user_id}/groups/${group_id}"
    echo "  ${username} → ${group_name}"
}

# Team Alpha: player, test2, test3
add_user_to_group "player" "${ALPHA_ID}" "team-alpha"
add_user_to_group "test2"  "${ALPHA_ID}" "team-alpha"
add_user_to_group "test3"  "${ALPHA_ID}" "team-alpha"

# Team Beta: test4, test5, test6
add_user_to_group "test4" "${BETA_ID}" "team-beta"
add_user_to_group "test5" "${BETA_ID}" "team-beta"
add_user_to_group "test6" "${BETA_ID}" "team-beta"

# Solo users (no team): test7, test8, test9, test10
echo ""
echo "  Solo users (no group): test7, test8, test9, test10"

# Admin user test1 - no team (tests admin in solo mode)
echo "  Admin (no group): test1"

echo ""
echo "=== Team setup complete! ==="

echo ""
echo "=== Updating team display names in DB ==="
PGPASSWORD=ctf psql -h localhost -U ctf -d ctf_app -c "
UPDATE teams SET team_name = 'Team Alpha' WHERE team_id = 'team-alpha';
UPDATE teams SET team_name = 'Team Beta'  WHERE team_id = 'team-beta';
" 2>/dev/null && echo "  Updated team names (Team Alpha, Team Beta)" \
             || echo "  (skipped — app not running or teams not yet created)"

echo ""
echo "Teams (via group membership → groups JWT claim):"
echo "  /ctf-teams/team-alpha: player, test2, test3"
echo "  /ctf-teams/team-beta:  test4, test5, test6"
echo "  Solo users: test7, test8, test9, test10"
echo "  Admin (solo): test1"
echo ""
echo "Login at http://localhost:8080 with any of these users (password = username)"
