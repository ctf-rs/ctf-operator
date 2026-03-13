use std::{sync::Arc, time::Duration};

use aws_credential_types::{Credentials, provider::SharedCredentialsProvider};
use aws_sdk_s3::{Client as S3Client, config::Region, presigning::PresigningConfig};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use tracing::warn;

use crate::{
    db,
    helpers::{current_user, has_ctf_access},
    s3::parse_s3_bucket_key,
    state::AppState,
};

pub async fn download_file(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path((challenge_name, file_idx)): Path<(String, usize)>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    let ctf_name = &state.config.ctf_name;

    let Some(challenge) = db::get_challenge(&state.db, ctf_name, &challenge_name)
        .await
        .ok()
        .flatten()
    else {
        return (axum::http::StatusCode::NOT_FOUND, "challenge not found").into_response();
    };

    if !has_ctf_access(&user, &challenge) {
        return (axum::http::StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let Some(file_url) = challenge.files.get(file_idx).cloned() else {
        return (axum::http::StatusCode::NOT_FOUND, "file not found").into_response();
    };

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
