use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Build a signed verification URL that the user opens in their browser.
///
/// The URL points to the web app's `/discord` endpoint and carries:
/// - `data`: base64url-encoded JSON `{"discord_id","discord_username","ts"}`
/// - `sig`:  hex-encoded HMAC-SHA256 of the `data` string
pub fn generate_verify_url(
    app_base_url: &str,
    secret: &str,
    discord_id: &str,
    discord_username: &str,
) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs();

    let payload = serde_json::json!({
        "discord_id": discord_id,
        "discord_username": discord_username,
        "ts": ts,
    });

    let json_bytes = serde_json::to_vec(&payload).expect("JSON serialisation cannot fail");
    let data = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&json_bytes);

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());

    format!(
        "{}/discord?data={}&sig={}",
        app_base_url.trim_end_matches('/'),
        data,
        sig
    )
}
