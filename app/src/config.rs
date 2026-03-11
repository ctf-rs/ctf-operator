use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub bind_addr: String,
    pub database_url: String,
    pub oidc: OidcConfig,
    pub watch_namespaces: Vec<String>,
    pub instance_name_prefix: String,
    pub session_cookie_name: String,
}

#[derive(Clone, Debug)]
pub struct OidcConfig {
    pub issuer_url: String,
    pub public_issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let bind_addr =
            std::env::var("APP_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
        let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
        let issuer_url = std::env::var("OIDC_ISSUER_URL").context("OIDC_ISSUER_URL is required")?;
        let public_issuer_url =
            std::env::var("OIDC_PUBLIC_ISSUER_URL").unwrap_or_else(|_| issuer_url.clone());
        let client_id = std::env::var("OIDC_CLIENT_ID").context("OIDC_CLIENT_ID is required")?;
        let client_secret =
            std::env::var("OIDC_CLIENT_SECRET").context("OIDC_CLIENT_SECRET is required")?;
        let redirect_url =
            std::env::var("OIDC_REDIRECT_URL").context("OIDC_REDIRECT_URL is required")?;

        let watch_namespaces = std::env::var("WATCH_NAMESPACES")
            .unwrap_or_else(|_| "ctf".to_string())
            .split(',')
            .map(str::trim)
            .filter(|ns| !ns.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        if watch_namespaces.is_empty() {
            anyhow::bail!("WATCH_NAMESPACES must contain at least one namespace");
        }

        let instance_name_prefix =
            std::env::var("INSTANCE_NAME_PREFIX").unwrap_or_else(|_| "app-inst".to_string());
        let session_cookie_name =
            std::env::var("SESSION_COOKIE_NAME").unwrap_or_else(|_| "ctf_app_session".to_string());

        Ok(Self {
            bind_addr,
            database_url,
            oidc: OidcConfig {
                issuer_url,
                public_issuer_url,
                client_id,
                client_secret,
                redirect_url,
            },
            watch_namespaces,
            instance_name_prefix,
            session_cookie_name,
        })
    }
}
