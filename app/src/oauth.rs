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
    name: Option<String>,
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

    Ok(UserClaims {
        sub: info.sub,
        email: info.email,
        name: info.name,
        roles,
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
        for (_client, value) in resource_access {
            if let Some(client_roles) = value.get("roles").and_then(|v| v.as_array()) {
                for role in client_roles.iter().filter_map(|v| v.as_str()) {
                    roles.push(role.to_string());
                }
            }
        }
    }

    roles.sort();
    roles.dedup();
    roles
}
