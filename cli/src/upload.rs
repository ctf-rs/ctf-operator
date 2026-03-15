use anyhow::{bail, Context};
use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, primitives::ByteStream, Client};
use std::{collections::HashSet, env, path::Path};

use crate::{cli::UploadArgs, read_challenge, read_ctf};

pub async fn upload(args: UploadArgs) -> anyhow::Result<()> {
    let UploadArgs {
        ctf,
        challenges,
        access_key_id,
        secret_access_key,
    } = args;

    let ctf_dir = ctf
        .parent()
        .context("ctf yaml must have parent directory")?
        .to_path_buf();

    let ctf = read_ctf(ctf).await?;

    let storage = ctf
        .storage
        .as_ref()
        .context("ctf.storage is required for upload")?;

    if ctf.challenges.is_empty() {
        bail!("ctf has no challenges configured");
    }

    let mut challenge_items = Vec::new();
    let mut seen_challenge_names = HashSet::<String>::new();
    for challenge_rel_path in &ctf.challenges {
        let challenge_path = ctf_dir.join(challenge_rel_path);
        let challenge = read_challenge(challenge_path.clone()).await?;

        if !seen_challenge_names.insert(challenge.name.clone()) {
            bail!(
                "duplicate challenge name in ctf challenge files: {}",
                challenge.name
            );
        }

        challenge_items.push((challenge_path, challenge));
    }

    let requested_names = challenges.into_iter().collect::<HashSet<_>>();
    let selected = if requested_names.is_empty() {
        challenge_items
    } else {
        let selected = challenge_items
            .into_iter()
            .filter(|(_, challenge)| requested_names.contains(&challenge.name))
            .collect::<Vec<_>>();

        let available = selected
            .iter()
            .map(|(_, challenge)| challenge.name.clone())
            .collect::<HashSet<_>>();
        let missing = requested_names
            .difference(&available)
            .cloned()
            .collect::<Vec<_>>();

        if !missing.is_empty() {
            bail!(
                "requested challenge name(s) not found in ctf challenge YAML names: {}",
                missing.join(", ")
            );
        }

        selected
    };

    let effective_endpoint = storage.endpoint.clone();
    let effective_region = storage
        .region
        .clone()
        .unwrap_or_else(|| "us-east-1".to_owned());

    let (access_key_id, secret_access_key) = resolve_credentials(access_key_id, secret_access_key)?;

    let client = build_s3_client(
        &effective_endpoint,
        &effective_region,
        storage.path_style,
        access_key_id,
        secret_access_key,
    )
    .await?;

    for (challenge_path, challenge) in selected {
        let challenge_dir = challenge_path
            .parent()
            .context("challenge yaml must have parent directory")?
            .to_path_buf();

        for rel_file in &challenge.files {
            let source_path = challenge_dir.join(rel_file);
            let file_name = rel_file
                .file_name()
                .with_context(|| format!("invalid file path in challenge: {}", rel_file.display()))?
                .to_string_lossy()
                .to_string();

            let key = format!("challenge/{}/{}", challenge.name, file_name);
            upload_file(&client, &storage.bucket, &key, &source_path).await?;
            println!(
                "Uploaded {} -> s3://{}/{}",
                source_path.display(),
                storage.bucket,
                key
            );
        }
    }

    Ok(())
}

async fn build_s3_client(
    endpoint: &str,
    region: &str,
    path_style: bool,
    access_key_id: String,
    secret_access_key: String,
) -> anyhow::Result<Client> {
    let mut loader =
        aws_config::defaults(BehaviorVersion::latest()).region(Region::new(region.to_string()));

    let creds = Credentials::new(
        access_key_id,
        secret_access_key,
        None,
        None,
        "ctf-cli-upload",
    );
    loader = loader.credentials_provider(creds);

    let shared = loader.load().await;
    let mut builder = aws_sdk_s3::config::Builder::from(&shared)
        .region(Region::new(region.to_string()))
        .endpoint_url(endpoint)
        .force_path_style(path_style)
        .aws_chunked_encoding_chunk_size(Some(8 * 1024))
        .request_checksum_calculation(aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired);

    if let Some(credentials_provider) = shared.credentials_provider() {
        builder = builder.credentials_provider(credentials_provider);
    }

    Ok(Client::from_conf(builder.build()))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "called once with owned values from CLI args"
)]
fn resolve_credentials(
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
) -> anyhow::Result<(String, String)> {
    match (&access_key_id, &secret_access_key) {
        (Some(_), None) | (None, Some(_)) => {
            bail!("both --access-key-id and --secret-access-key must be provided together")
        }
        (Some(key), Some(secret)) => return Ok((key.clone(), secret.clone())),
        (None, None) => {}
    }

    let env_key = env::var("AWS_ACCESS_KEY_ID")
        .ok()
        .or_else(|| env::var("S3_ACCESS_KEY_ID").ok());
    let env_secret = env::var("AWS_SECRET_ACCESS_KEY")
        .ok()
        .or_else(|| env::var("S3_SECRET_ACCESS_KEY").ok());

    match (&env_key, &env_secret) {
        (Some(key), Some(secret)) => Ok((key.clone(), secret.clone())),
        (Some(_), None) | (None, Some(_)) => {
            bail!("incomplete credentials in environment: both AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY are required")
        }
        (None, None) => bail!(
            "S3 credentials are required: provide --access-key-id and --secret-access-key, or set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY"
        ),
    }
}

async fn upload_file(
    client: &Client,
    bucket: &str,
    key: &str,
    source_path: &Path,
) -> anyhow::Result<()> {
    let body = ByteStream::from_path(source_path.to_path_buf())
        .await
        .with_context(|| format!("failed reading file {}", source_path.display()))?;

    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body)
        .send()
        .await
        .with_context(|| {
            format!(
                "failed uploading {} to s3://{}/{}",
                source_path.display(),
                bucket,
                key
            )
        })?;

    Ok(())
}
