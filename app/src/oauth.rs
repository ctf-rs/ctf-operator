use anyhow::Result;
use base64::Engine;
use rand::RngExt;
use serde::Deserialize;
use serde_json::Value;
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
    pub team_name: Option<String>,
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

    Ok(OidcClient {
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
        redirect_url: config.redirect_url.clone(),
        auth_url,
        token_url,
        userinfo_url,
        http: reqwest::Client::builder().build()?,
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
    let (team_id, team_name) = extract_team_from_jwt(&token.access_token);

    Ok(UserClaims {
        sub: info.sub,
        email: info.email,
        name: info.preferred_username,
        roles,
        team_id,
        team_name,
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

/// Extract `team_id` and `team_name` from a Keycloak JWT access token.
///
/// Expects custom claims added via a Keycloak protocol mapper:
/// - `team_id`: unique identifier for the team (e.g. Keycloak group ID or attribute)
/// - `team_name`: human-readable team name
fn extract_team_from_jwt(access_token: &str) -> (Option<String>, Option<String>) {
    let parts = access_token.split('.').collect::<Vec<_>>();
    if parts.len() < 2 {
        return (None, None);
    }

    let Ok(decoded) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]) else {
        return (None, None);
    };

    let Ok(json) = serde_json::from_slice::<Value>(&decoded) else {
        return (None, None);
    };

    let team_id = json.get("team_id").and_then(|v| v.as_str()).map(String::from);
    let team_name = json
        .get("team_name")
        .and_then(|v| v.as_str())
        .map(String::from);

    (team_id, team_name)
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

    #[test]
    fn extract_team_from_jwt_with_both_claims() {
        let payload = serde_json::json!({
            "sub": "user-1",
            "team_id": "team-alpha",
            "team_name": "Alpha Squad",
        });
        let token = fake_jwt(&payload);
        let (team_id, team_name) = extract_team_from_jwt(&token);
        assert_eq!(team_id.as_deref(), Some("team-alpha"));
        assert_eq!(team_name.as_deref(), Some("Alpha Squad"));
    }

    #[test]
    fn extract_team_from_jwt_without_team_claims() {
        let payload = serde_json::json!({
            "sub": "user-1",
            "email": "test@example.com",
        });
        let token = fake_jwt(&payload);
        let (team_id, team_name) = extract_team_from_jwt(&token);
        assert!(team_id.is_none());
        assert!(team_name.is_none());
    }

    #[test]
    fn extract_team_from_jwt_with_only_team_id() {
        let payload = serde_json::json!({
            "sub": "user-1",
            "team_id": "team-beta",
        });
        let token = fake_jwt(&payload);
        let (team_id, team_name) = extract_team_from_jwt(&token);
        assert_eq!(team_id.as_deref(), Some("team-beta"));
        assert!(team_name.is_none());
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
