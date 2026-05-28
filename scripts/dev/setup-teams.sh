#!/usr/bin/env bash
# Configure Keycloak for team mode testing.
# Adds team groups, a team_id/team_name mapper, and assigns test users to teams.
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
# Helper: POST with auth
kc_post() { curl -sf -X POST -H "${AUTH}" -H "Content-Type: application/json" -d "$2" "${KC_URL}/admin/realms/${KC_REALM}$1"; }
# Helper: PUT with auth
kc_put() { curl -sf -X PUT -H "${AUTH}" -H "Content-Type: application/json" -d "$2" "${KC_URL}/admin/realms/${KC_REALM}$1"; }

echo ""
echo "=== Setting up protocol mappers for team claims ==="

# Get client internal ID
CLIENT_ID=$(kc_get "/clients" | python3 -c "
import sys, json
for c in json.load(sys.stdin):
    if c['clientId'] == 'ctf-app':
        print(c['id'])
        break
")
echo "  ctf-app client id: ${CLIENT_ID}"

# Add team_id mapper (group ID → team_id claim)
# We use a hardcoded attribute mapper - users get team_id set as a user attribute
# when they join a team group. A script mapper would be more elegant but requires
# Keycloak scripting enabled.

# Instead, we'll use a simple approach: set user attributes directly.
# The app reads team_id and team_name from the JWT token.

# Create a user attribute mapper for team_id
MAPPER_EXISTS=$(kc_get "/clients/${CLIENT_ID}/protocol-mappers/models" | python3 -c "
import sys, json
for m in json.load(sys.stdin):
    if m.get('name') == 'team_id':
        print('yes')
        break
" 2>/dev/null || true)

if [ "${MAPPER_EXISTS}" != "yes" ]; then
    kc_post "/clients/${CLIENT_ID}/protocol-mappers/models" '{
        "name": "team_id",
        "protocol": "openid-connect",
        "protocolMapper": "oidc-usermodel-attribute-mapper",
        "config": {
            "user.attribute": "team_id",
            "claim.name": "team_id",
            "jsonType.label": "String",
            "id.token.claim": "true",
            "access.token.claim": "true",
            "userinfo.token.claim": "true",
            "multivalued": "false"
        }
    }'
    echo "  Created team_id mapper"
else
    echo "  team_id mapper already exists"
fi

MAPPER_EXISTS=$(kc_get "/clients/${CLIENT_ID}/protocol-mappers/models" | python3 -c "
import sys, json
for m in json.load(sys.stdin):
    if m.get('name') == 'team_name':
        print('yes')
        break
" 2>/dev/null || true)

if [ "${MAPPER_EXISTS}" != "yes" ]; then
    kc_post "/clients/${CLIENT_ID}/protocol-mappers/models" '{
        "name": "team_name",
        "protocol": "openid-connect",
        "protocolMapper": "oidc-usermodel-attribute-mapper",
        "config": {
            "user.attribute": "team_name",
            "claim.name": "team_name",
            "jsonType.label": "String",
            "id.token.claim": "true",
            "access.token.claim": "true",
            "userinfo.token.claim": "true",
            "multivalued": "false"
        }
    }'
    echo "  Created team_name mapper"
else
    echo "  team_name mapper already exists"
fi

echo ""
echo "=== Registering team attributes in user profile (Keycloak 26+) ==="
# Keycloak 26 requires custom attributes to be registered in the user profile
# before they can be set on users.
PROFILE=$(kc_get "/users/profile")
NEEDS_UPDATE=$(echo "${PROFILE}" | python3 -c "
import sys, json
profile = json.load(sys.stdin)
names = [a['name'] for a in profile['attributes']]
if 'team_id' not in names or 'team_name' not in names:
    print('yes')
else:
    print('no')
")

if [ "${NEEDS_UPDATE}" = "yes" ]; then
    UPDATED_PROFILE=$(echo "${PROFILE}" | python3 -c "
import sys, json
profile = json.load(sys.stdin)
names = [a['name'] for a in profile['attributes']]
if 'team_id' not in names:
    profile['attributes'].append({
        'name': 'team_id',
        'displayName': 'Team ID',
        'permissions': {'view': ['admin'], 'edit': ['admin']},
        'multivalued': False
    })
if 'team_name' not in names:
    profile['attributes'].append({
        'name': 'team_name',
        'displayName': 'Team Name',
        'permissions': {'view': ['admin'], 'edit': ['admin']},
        'multivalued': False
    })
# Remove required constraint from email/firstName/lastName for test users
for attr in profile['attributes']:
    if attr['name'] in ('email', 'firstName', 'lastName'):
        attr.pop('required', None)
print(json.dumps(profile))
")
    kc_put "/users/profile" "${UPDATED_PROFILE}"
    echo "  Registered team_id and team_name attributes"
else
    echo "  Attributes already registered"
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
echo "=== Assigning users to teams ==="

# Helper: get user ID by username
get_user_id() {
    kc_get "/users?username=$1&exact=true" | python3 -c "
import sys, json
users = json.load(sys.stdin)
if users: print(users[0]['id'])
"
}

# Helper: set user attributes
set_user_team() {
    local username="$1"
    local team_id="$2"
    local team_name="$3"
    
    local user_id
    user_id=$(get_user_id "${username}")
    if [ -z "${user_id}" ]; then
        echo "  WARNING: user '${username}' not found"
        return
    fi
    
    # Get current user, update attributes
    local user_json
    user_json=$(kc_get "/users/${user_id}")
    
    local updated
    updated=$(echo "${user_json}" | python3 -c "
import sys, json
u = json.load(sys.stdin)
attrs = u.get('attributes', {})
attrs['team_id'] = ['${team_id}']
attrs['team_name'] = ['${team_name}']
u['attributes'] = attrs
print(json.dumps(u))
")
    
    kc_put "/users/${user_id}" "${updated}"
    echo "  ${username} → ${team_name} (${team_id})"
}

# Team Alpha: player, test2, test3
set_user_team "player" "team-alpha" "Team Alpha"
set_user_team "test2"  "team-alpha" "Team Alpha"
set_user_team "test3"  "team-alpha" "Team Alpha"

# Team Beta: test4, test5, test6
set_user_team "test4" "team-beta" "Team Beta"
set_user_team "test5" "team-beta" "Team Beta"
set_user_team "test6" "team-beta" "Team Beta"

# Solo users (no team): test7, test8, test9, test10
echo ""
echo "  Solo users (no team): test7, test8, test9, test10"

# Admin user test1 - no team (tests admin in solo mode)
echo "  Admin (no team): test1"

echo ""
echo "=== Team setup complete! ==="
echo ""
echo "Teams:"
echo "  Team Alpha (team-alpha): player, test2, test3"
echo "  Team Beta  (team-beta): test4, test5, test6"
echo "  Solo users: test7, test8, test9, test10"
echo "  Admin (solo): test1"
echo ""
echo "Login at http://localhost:8090 with any of these users (password = username)"
