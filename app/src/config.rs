use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub bind_addr: String,
    pub database_url: String,
    pub oidc: OidcConfig,
    pub ctf_name: String,
    pub ctf_namespace: String,
    pub instance_name_prefix: String,
    pub session_cookie_name: String,
    pub challenge_files_s3_endpoint: Option<String>,
    pub challenge_files_s3_path_style: bool,
    pub challenge_files_s3_region: String,
    pub challenge_files_presign_ttl_seconds: u64,
    pub challenge_files_s3_access_key_id: Option<String>,
    pub challenge_files_s3_secret_access_key: Option<String>,
    pub challenge_files_s3_session_token: Option<String>,
    /// Base domain for instance connections (e.g., "ctf.local").
    pub instance_base_domain: Option<String>,
    /// If true, extract client IP from proxy headers (X-Forwarded-For, etc.).
    /// If false, use the socket connection IP directly.
    pub behind_reverse_proxy: bool,
    /// Shared HMAC secret used by the Discord bot to sign account-link tokens.
    /// When unset, the `/discord` linking route is disabled.
    pub discord_link_secret: Option<String>,
    /// URL of the Discord bot's HTTP callback server (e.g. `http://localhost:3030`).
    /// When set, the app POSTs a signed notification after a successful account link
    /// so the bot can automatically assign the competitor role.
    pub discord_bot_callback_url: Option<String>,
    /// OIDC/JWT role name that grants admin access.  Admins can always view
    /// non-public CTFs, and their flag submissions are excluded from the
    /// scoreboard.
    pub admin_role: Option<String>,
    /// OIDC/JWT role name that grants player (competitor) access to non-public
    /// CTFs.  When a CTF is not public, a user must hold this role (or the
    /// admin role) to view challenges.
    pub player_role: Option<String>,
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

        let ctf_name = std::env::var("CTF_NAME").context("CTF_NAME is required")?;
        let ctf_namespace = std::env::var("CTF_NAMESPACE").unwrap_or_else(|_| "ctf".to_string());

        let instance_name_prefix =
            std::env::var("INSTANCE_NAME_PREFIX").unwrap_or_else(|_| "ctf-inst".to_string());
        let session_cookie_name =
            std::env::var("SESSION_COOKIE_NAME").unwrap_or_else(|_| "ctf_app_session".to_string());
        let challenge_files_s3_endpoint = std::env::var("CHALLENGE_FILES_S3_ENDPOINT")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let challenge_files_s3_path_style = std::env::var("CHALLENGE_FILES_S3_PATH_STYLE")
            .ok()
            .as_deref()
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true);
        let challenge_files_s3_region =
            std::env::var("CHALLENGE_FILES_S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let challenge_files_presign_ttl_seconds =
            std::env::var("CHALLENGE_FILES_PRESIGN_TTL_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(600);
        let challenge_files_s3_access_key_id = std::env::var("CHALLENGE_FILES_S3_ACCESS_KEY_ID")
            .ok()
            .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let challenge_files_s3_secret_access_key =
            std::env::var("CHALLENGE_FILES_S3_SECRET_ACCESS_KEY")
                .ok()
                .or_else(|| std::env::var("AWS_SECRET_ACCESS_KEY").ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
        let challenge_files_s3_session_token = std::env::var("CHALLENGE_FILES_S3_SESSION_TOKEN")
            .ok()
            .or_else(|| std::env::var("AWS_SESSION_TOKEN").ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let instance_base_domain = std::env::var("INSTANCE_BASE_DOMAIN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let behind_reverse_proxy = std::env::var("BEHIND_REVERSE_PROXY")
            .ok()
            .as_deref()
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);

        let discord_link_secret = std::env::var("DISCORD_LINK_SECRET")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let discord_bot_callback_url = std::env::var("DISCORD_BOT_CALLBACK_URL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let admin_role = std::env::var("OIDC_ADMIN_ROLE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let player_role = std::env::var("OIDC_PLAYER_ROLE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

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
            ctf_name,
            ctf_namespace,
            instance_name_prefix,
            session_cookie_name,
            challenge_files_s3_endpoint,
            challenge_files_s3_path_style,
            challenge_files_s3_region,
            challenge_files_presign_ttl_seconds,
            challenge_files_s3_access_key_id,
            challenge_files_s3_secret_access_key,
            challenge_files_s3_session_token,
            instance_base_domain,
            behind_reverse_proxy,
            discord_link_secret,
            discord_bot_callback_url,
            admin_role,
            player_role,
        })
    }
}
