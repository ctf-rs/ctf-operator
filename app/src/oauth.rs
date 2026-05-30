use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use base64::Engine;
use rand::RngExt;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;
use url::Url;

use crate::config::OidcConfig;

#[derive(Debug, Clone)]
pub struct PendingAuth {
    pub nonce: String,
}

#[derive(Debug, Clone)]
pub struct UserClaims {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub roles: Vec<String>,
    pub team_id: Option<String>,
}

/// Resolves Keycloak group paths (from the `groups` JWT claim) to team IDs
/// set as group attributes.
///
/// At startup the resolver fetches the full group tree from Keycloak's
/// admin API and caches a map of `group_path → team_id`.  A background
/// refresh can be triggered with [`GroupTeamResolver::refresh`].
#[derive(Debug, Clone)]
pub struct GroupTeamResolver {
    /// group path (e.g. "/scc2026-finals/team1") → team_id
    cache: Arc<RwLock<HashMap<String, String>>>,
    admin_url: String,
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    token_url: String,
}

#[derive(Debug, Deserialize)]
struct KeycloakGroup {
    id: String,
    // name: String,
    path: String,
    #[serde(default)]
    attributes: HashMap<String, Vec<String>>,
    #[serde(default, rename = "subGroups")]
    sub_groups: Vec<KeycloakGroup>,
    #[serde(default, rename = "subGroupCount")]
    sub_group_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ServiceTokenResponse {
    access_token: String,
}

impl GroupTeamResolver {
    /// Build a new resolver. Fetches the group tree once at construction.
    pub async fn new(config: &OidcConfig) -> Result<Self> {
        let issuer = config.issuer_url.trim_end_matches('/');
        // Derive admin API base from issuer URL:
        //   http://localhost:8081/realms/ctf  → http://localhost:8081/admin/realms/ctf
        let admin_url = issuer.replacen("/realms/", "/admin/realms/", 1);
        let token_url = format!("{issuer}/protocol/openid-connect/token");

        let resolver = Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            admin_url,
            http: reqwest::Client::new(),
            client_id: config.client_id.clone(),
            client_secret: config.client_secret.clone(),
            token_url,
        };

        if let Err(e) = resolver.refresh().await {
            tracing::warn!("failed to load group→team mapping (will retry on login): {e}");
        }

        Ok(resolver)
    }

    /// Fetch a service-account token for the OIDC client.
    async fn get_admin_token(&self) -> Result<String> {
        let resp = self
            .http
            .post(&self.token_url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<ServiceTokenResponse>()
            .await?;
        Ok(resp.access_token)
    }

    /// Refresh the cached group→team mapping from Keycloak.
    pub async fn refresh(&self) -> Result<()> {
        let token = self.get_admin_token().await?;

        // Fetch top-level groups
        let top_groups: Vec<KeycloakGroup> = self
            .http
            .get(format!("{}/groups?briefRepresentation=false&max=1000", self.admin_url))
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut map = HashMap::new();
        for g in &top_groups {
            self.walk_group(g, &mut map, &token).await?;
        }

        let count = map.len();
        *self.cache.write().await = map;
        tracing::info!(count, "group→team cache refreshed");
        Ok(())
    }

    /// Recursively walk a group tree, collecting team_id attributes.
    fn walk_group<'a>(
        &'a self,
        group: &'a KeycloakGroup,
        map: &'a mut HashMap<String, String>,
        token: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            // Check if this group has a team_id attribute
            if let Some(tid) = group
                .attributes
                .get("team_id")
                .and_then(|v| v.first())
                .filter(|s| !s.is_empty())
            {
                map.insert(group.path.clone(), tid.clone());
            }

            // Process inline sub_groups
            for child in &group.sub_groups {
                self.walk_group(child, map, token).await?;
            }

            // If the group reports having children but sub_groups is empty,
            // fetch them via the /children endpoint.
            if group.sub_groups.is_empty() && group.sub_group_count.unwrap_or(0) > 0 {
                let children: Vec<KeycloakGroup> = self
                    .http
                    .get(format!(
                        "{}/groups/{}/children?briefRepresentation=false&max=1000",
                        self.admin_url, group.id
                    ))
                    .bearer_auth(token)
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                for child in &children {
                    self.walk_group(child, map, token).await?;
                }
            }

            Ok(())
        })
    }

    /// Look up team_id for a list of group paths from a JWT `groups` claim.
    pub async fn resolve(&self, group_paths: &[String]) -> Option<String> {
        let cache = self.cache.read().await;
        for path in group_paths {
            if let Some(team_id) = cache.get(path.as_str()) {
                return Some(team_id.clone());
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct OidcClient {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
    pub auth_url: Url,
    pub token_url: Url,
    pub userinfo_url: Url,
    pub http: reqwest::Client,
    pub group_resolver: GroupTeamResolver,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    sub: String,
    email: Option<String>,
    preferred_username: Option<String>,
}

pub async fn build_client(config: &OidcConfig) -> Result<OidcClient> {
    let internal_issuer = config.issuer_url.trim_end_matches('/');
    let public_issuer = config.public_issuer_url.trim_end_matches('/');
    let auth_url = Url::parse(&format!("{public_issuer}/protocol/openid-connect/auth"))?;
    let token_url = Url::parse(&format!("{internal_issuer}/protocol/openid-connect/token"))?;
    let userinfo_url = Url::parse(&format!(
        "{internal_issuer}/protocol/openid-connect/userinfo"
    ))?;

    let group_resolver = GroupTeamResolver::new(config).await?;

    Ok(OidcClient {
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
        redirect_url: config.redirect_url.clone(),
        auth_url,
        token_url,
        userinfo_url,
        http: reqwest::Client::builder().build()?,
        group_resolver,
    })
}

pub fn begin_login(client: &OidcClient) -> Result<(Url, String, String)> {
    let state = random_token(24);
    let nonce = random_token(24);

    let mut auth_url = client.auth_url.clone();
    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", &client.redirect_url)
        .append_pair("scope", "openid profile email")
        .append_pair("state", &state)
        .append_pair("nonce", &nonce);

    Ok((auth_url, state, nonce))
}

pub async fn finish_login(client: &OidcClient, code: String) -> Result<UserClaims> {
    let token = client
        .http
        .post(client.token_url.clone())
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("client_id", &client.client_id),
            ("client_secret", &client.client_secret),
            ("redirect_uri", &client.redirect_url),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<TokenResponse>()
        .await?;

    let info = client
        .http
        .get(client.userinfo_url.clone())
        .bearer_auth(&token.access_token)
        .send()
        .await?
        .error_for_status()?
        .json::<UserInfoResponse>()
        .await?;

    let roles = extract_roles_from_jwt(&token.access_token);
    let team_id = extract_team_from_jwt(&token.access_token, &client.group_resolver).await;

    Ok(UserClaims {
        sub: info.sub,
        email: info.email,
        name: info.preferred_username,
        roles,
        team_id,
    })
}

fn random_token(len: usize) -> String {
    const ALPHABET: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..ALPHABET.len());
            ALPHABET.as_bytes()[idx] as char
        })
        .collect()
}

fn extract_roles_from_jwt(access_token: &str) -> Vec<String> {
    let mut roles = Vec::new();
    let parts = access_token.split('.').collect::<Vec<_>>();
    if parts.len() < 2 {
        return roles;
    }

    let payload = parts[1];
    let Ok(decoded) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload) else {
        return roles;
    };

    let Ok(json) = serde_json::from_slice::<Value>(&decoded) else {
        return roles;
    };

    if let Some(realm_roles) = json
        .get("realm_access")
        .and_then(|v| v.get("roles"))
        .and_then(|v| v.as_array())
    {
        for role in realm_roles.iter().filter_map(|v| v.as_str()) {
            roles.push(role.to_string());
        }
    }

    if let Some(resource_access) = json.get("resource_access").and_then(|v| v.as_object()) {
        for (client, value) in resource_access {
            if let Some(client_roles) = value.get("roles").and_then(|v| v.as_array()) {
                for role in client_roles.iter().filter_map(|v| v.as_str()) {
                    // Client roles are stored as `<client>/<role>` to
                    // avoid collisions with realm roles or roles from
                    // other clients (e.g. "scc2026-quals/admin").
                    roles.push(format!("{client}/{role}"));
                }
            }
        }
    }

    roles.sort();
    roles.dedup();
    roles
}

/// Extract `team_id` from a Keycloak JWT access token.
///
/// Supports two modes:
/// 1. **Group attributes** (preferred): reads the `groups` claim (array of group
///    paths like `["/scc2026-finals/team1"]`) and resolves team_id by looking
///    up the `team_id` attribute on those groups via `GroupTeamResolver`.
/// 2. **Explicit claim** (fallback): reads `team_id` claim directly.
async fn extract_team_from_jwt(
    access_token: &str,
    resolver: &GroupTeamResolver,
) -> Option<String> {
    let parts = access_token.split('.').collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }

    let Ok(decoded) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]) else {
        return None;
    };

    let Ok(json) = serde_json::from_slice::<Value>(&decoded) else {
        return None;
    };

    // Try group-based resolution first
    if let Some(groups) = json.get("groups").and_then(|v| v.as_array()) {
        let paths: Vec<String> = groups
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if let Some(team_id) = resolver.resolve(&paths).await {
            return Some(team_id);
        }
    }

    // Fallback to explicit claim
    json.get("team_id")
        .and_then(|v| v.as_str())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    /// Build a fake JWT (header.payload.signature) from a JSON payload.
    fn fake_jwt(payload: &serde_json::Value) -> String {
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(b"{\"alg\":\"RS256\",\"typ\":\"JWT\"}");
        let body = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(payload).unwrap());
        format!("{header}.{body}.fakesig")
    }

    /// Build a test resolver pre-loaded with a group→team_id mapping.
    fn test_resolver(entries: &[(&str, &str)]) -> GroupTeamResolver {
        let mut map = HashMap::new();
        for (path, team_id) in entries {
            map.insert(path.to_string(), team_id.to_string());
        }
        GroupTeamResolver {
            cache: Arc::new(RwLock::new(map)),
            admin_url: String::new(),
            http: reqwest::Client::new(),
            client_id: String::new(),
            client_secret: String::new(),
            token_url: String::new(),
        }
    }

    #[tokio::test]
    async fn extract_team_from_jwt_with_explicit_claim() {
        let resolver = test_resolver(&[]);
        let payload = serde_json::json!({
            "sub": "user-1",
            "team_id": "team-alpha",
        });
        let token = fake_jwt(&payload);
        let team_id = extract_team_from_jwt(&token, &resolver).await;
        assert_eq!(team_id.as_deref(), Some("team-alpha"));
    }

    #[tokio::test]
    async fn extract_team_from_groups_via_resolver() {
        let resolver = test_resolver(&[
            ("/scc2026-finals/team3", "team3"),
            ("/scc2026-finals/team17", "team17"),
        ]);
        let payload = serde_json::json!({
            "sub": "user-1",
            "groups": ["/scc2026-finals/team3"],
        });
        let token = fake_jwt(&payload);
        let team_id = extract_team_from_jwt(&token, &resolver).await;
        assert_eq!(team_id.as_deref(), Some("team3"));
    }

    #[tokio::test]
    async fn extract_team_from_groups_prefers_group_over_explicit() {
        let resolver = test_resolver(&[
            ("/scc2026-finals/team17", "team17"),
        ]);
        let payload = serde_json::json!({
            "sub": "user-1",
            "groups": ["/scc2026-finals/team17"],
            "team_id": "old-id",
        });
        let token = fake_jwt(&payload);
        let team_id = extract_team_from_jwt(&token, &resolver).await;
        assert_eq!(team_id.as_deref(), Some("team17"));
    }

    #[tokio::test]
    async fn extract_team_groups_not_in_resolver_falls_back() {
        let resolver = test_resolver(&[]);
        let payload = serde_json::json!({
            "sub": "user-1",
            "groups": ["/scc2026-finals", "/some-other-group"],
            "team_id": "fallback",
        });
        let token = fake_jwt(&payload);
        let team_id = extract_team_from_jwt(&token, &resolver).await;
        assert_eq!(team_id.as_deref(), Some("fallback"));
    }

    #[tokio::test]
    async fn extract_team_hyphenated_group() {
        let resolver = test_resolver(&[
            ("/ctf-teams/team-alpha", "team-alpha"),
        ]);
        let payload = serde_json::json!({
            "sub": "user-1",
            "groups": ["/ctf-teams/team-alpha"],
        });
        let token = fake_jwt(&payload);
        let team_id = extract_team_from_jwt(&token, &resolver).await;
        assert_eq!(team_id.as_deref(), Some("team-alpha"));
    }

    #[tokio::test]
    async fn extract_team_from_jwt_without_team_claims() {
        let resolver = test_resolver(&[]);
        let payload = serde_json::json!({
            "sub": "user-1",
            "email": "test@example.com",
        });
        let token = fake_jwt(&payload);
        let team_id = extract_team_from_jwt(&token, &resolver).await;
        assert!(team_id.is_none());
    }

    #[tokio::test]
    async fn extract_team_from_jwt_with_only_team_id() {
        let resolver = test_resolver(&[]);
        let payload = serde_json::json!({
            "sub": "user-1",
            "team_id": "team-beta",
        });
        let token = fake_jwt(&payload);
        let team_id = extract_team_from_jwt(&token, &resolver).await;
        assert_eq!(team_id.as_deref(), Some("team-beta"));
    }

    #[test]
    fn extract_roles_from_jwt_realm_and_client() {
        let payload = serde_json::json!({
            "realm_access": { "roles": ["admin", "user"] },
            "resource_access": {
                "my-client": { "roles": ["editor"] }
            }
        });
        let token = fake_jwt(&payload);
        let roles = extract_roles_from_jwt(&token);
        assert!(roles.contains(&"admin".to_string()));
        assert!(roles.contains(&"user".to_string()));
        assert!(roles.contains(&"my-client/editor".to_string()));
    }

    #[test]
    fn extract_roles_from_invalid_jwt_returns_empty() {
        let roles = extract_roles_from_jwt("not-a-jwt");
        assert!(roles.is_empty());
    }
}
