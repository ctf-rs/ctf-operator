use std::{net::SocketAddr, sync::Arc, time::Duration};

use aws_credential_types::{Credentials, provider::SharedCredentialsProvider};
use aws_sdk_s3::{Client as S3Client, config::Region, presigning::PresigningConfig};
use axum::{
    Extension,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use tracing::warn;

use crate::{
    db,
    helpers::{extract_client_ip, has_ctf_access},
    k8s::challenges,
    metrics,
    s3::parse_s3_bucket_key,
    state::AppState,
};

pub async fn download_file(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<db::User>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path((challenge_name, file_idx)): Path<(String, usize)>,
) -> Response {
    let ctf_name = &state.config.ctf_name;
    let client_ip = extract_client_ip(state.config.behind_reverse_proxy, &headers, Some(addr));

    let Some(challenge) = challenges::get_challenge(&state, &challenge_name) else {
        return (axum::http::StatusCode::NOT_FOUND, "challenge not found").into_response();
    };

    if !has_ctf_access(&user, &challenge, &state.config) {
        return (axum::http::StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let Some(file_url) = challenge.files.get(file_idx).cloned() else {
        return (axum::http::StatusCode::NOT_FOUND, "file not found").into_response();
    };

    // Audit log: file download
    let m = metrics::get();
    m.file_downloads
        .add(1, &m.attrs(&[metrics::challenge_kv(&challenge_name)]));
    if let Err(err) = db::log_download(
        &state.db,
        user.id,
        &challenge_name,
        ctf_name,
        &file_url,
        client_ip.as_deref(),
    )
    .await
    {
        warn!(error = %err, "failed to log download audit");
    }

    let Some((bucket, key)) = parse_s3_bucket_key(
        &file_url,
        state.config.challenge_files_s3_endpoint.as_deref(),
    ) else {
        return Redirect::to(&file_url).into_response();
    };

    let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new(state.config.challenge_files_s3_region.clone()))
        .load()
        .await;

    let mut s3_config = aws_sdk_s3::config::Builder::from(&sdk_config)
        .force_path_style(state.config.challenge_files_s3_path_style);

    if let (Some(access_key_id), Some(secret_access_key)) = (
        state.config.challenge_files_s3_access_key_id.as_deref(),
        state.config.challenge_files_s3_secret_access_key.as_deref(),
    ) {
        let credentials = Credentials::new(
            access_key_id,
            secret_access_key,
            state.config.challenge_files_s3_session_token.clone(),
            None,
            "app-config",
        );
        s3_config = s3_config.credentials_provider(SharedCredentialsProvider::new(credentials));
    }

    if let Some(endpoint) = state.config.challenge_files_s3_endpoint.as_deref() {
        s3_config = s3_config.endpoint_url(endpoint);
    }

    let client = S3Client::from_conf(s3_config.build());

    let presign_config = match PresigningConfig::expires_in(Duration::from_secs(
        state.config.challenge_files_presign_ttl_seconds,
    )) {
        Ok(value) => value,
        Err(err) => {
            warn!(error = %err, "invalid presign ttl");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "invalid presign configuration",
            )
                .into_response();
        }
    };

    match client
        .get_object()
        .bucket(bucket)
        .key(key)
        .presigned(presign_config)
        .await
    {
        Ok(presigned) => Redirect::to(&presigned.uri().to_string()).into_response(),
        Err(err) => {
            warn!(error = %err, "failed to create presigned file URL");
            Redirect::to(&file_url).into_response()
        }
    }
}
