use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aws_credential_types::{Credentials, provider::SharedCredentialsProvider};
use aws_sdk_s3::{Client as S3Client, config::Region, presigning::PresigningConfig};

use axum::{
    extract::{Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    Json,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use ctf_cli::k8s::{Challenge, Ctf, Flag, Instance, InstanceSpec};
use ctf_cli::models::Flag as CoreFlag;
use kube::{
    Api,
    api::{Patch, PatchParams},
};
use serde::{Deserialize, Serialize};
use tracing::warn;
use url::Url;
use uuid::Uuid;

use crate::{
    AppState,
    db::{self, ChallengeView, User},
    oauth,
};

#[derive(Deserialize)]
pub struct OauthCallbackQuery {
    code: String,
    state: String,
}

#[derive(Deserialize)]
pub struct SubmitFlagForm {
    flag_name: String,
    submitted_flag: String,
}

#[derive(Deserialize)]
pub struct ChallengePageQuery {
    submit: Option<String>,
    flag: Option<String>,
    message: Option<String>,
    extend: Option<String>,
}

#[derive(Serialize)]
pub struct InstanceStateResponse {
    has_instance: bool,
    ready: bool,
    can_extend: bool,
    solved: bool,
    time_left_seconds: Option<u64>,
    instance_name: Option<String>,
}

pub async fn healthz() -> impl IntoResponse {
    "ok"
}

pub async fn login(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let Ok((auth_url, state_token, nonce)) = oauth::begin_login(&state.oidc_client) else {
        return Redirect::to("/");
    };

    state
        .pending_oauth
        .write()
        .await
        .insert(state_token, oauth::PendingAuth { nonce });

    Redirect::to(auth_url.as_str())
}

pub async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(query): Query<OauthCallbackQuery>,
) -> Response {
    let pending = state.pending_oauth.write().await.remove(&query.state);
    let Some(_pending) = pending else {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid oauth state").into_response();
    };

    let claims = match oauth::finish_login(&state.oidc_client, query.code).await {
        Ok(claims) => claims,
        Err(err) => {
            warn!(error = %err, "oauth callback failed");
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "oauth callback failed",
            )
                .into_response();
        }
    };

    let user = match db::upsert_user(
        &state.db,
        &claims.sub,
        claims.email.as_deref(),
        claims.name.as_deref(),
        &claims.roles,
    )
    .await
    {
        Ok(user) => user,
        Err(err) => {
            warn!(error = %err, "failed to upsert user");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "failed to persist user",
            )
                .into_response();
        }
    };

    let token = Uuid::new_v4().to_string();
    if let Err(err) = db::create_session(&state.db, &token, user.id).await {
        warn!(error = %err, "failed to create session");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create session",
        )
            .into_response();
    }

    let cookie = Cookie::build((state.config.session_cookie_name.clone(), token))
        .path("/")
        .http_only(true)
        .secure(false)
        .build();

    (jar.add(cookie), Redirect::to("/")).into_response()
}

pub async fn logout(State(state): State<Arc<AppState>>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(&state.config.session_cookie_name) {
        let _ = db::delete_session(&state.db, cookie.value()).await;
    }

    let cookie = Cookie::build((state.config.session_cookie_name.clone(), String::new()))
        .path("/")
        .build();

    (jar.remove(cookie), Redirect::to("/")).into_response()
}

pub async fn index(State(state): State<Arc<AppState>>, jar: CookieJar) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(user) => user,
        Err(err) => {
            warn!(error = %err, "failed to read current user");
            None
        }
    };

    let Some(user) = user else {
        let html = render_page(
            "CTF App",
            "<div class=\"auth-card\"><h1>CTF App</h1><p>Login required to view challenges.</p><a class=\"btn\" href=\"/login\">Login with Keycloak</a></div>",
        );
        return Html(html).into_response();
    };

    let challenges = match db::list_challenges(&state.db).await {
        Ok(challenges) => challenges,
        Err(err) => {
            warn!(error = %err, "failed to list challenges");
            Vec::new()
        }
    };

    let cards = challenges
        .into_iter()
        .filter(|challenge| has_ctf_access(&user, challenge))
        .collect::<Vec<_>>();

    let solved_keys = db::list_solved_challenges_for_user(&state.db, user.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|entry| format!("{}::{}", entry.ctf_name, entry.challenge_name))
        .collect::<HashSet<_>>();

    let cards = cards
        .into_iter()
        .map(|challenge| {
            let solved = solved_keys.contains(&format!("{}::{}", challenge.ctf_name, challenge.name));
            ChallengeCardView { challenge, solved }
        })
        .collect::<Vec<_>>();

    let user_label = user
        .name
        .clone()
        .or(user.email.clone())
        .unwrap_or_else(|| user.sub.clone());

    let cards_html = cards
        .iter()
        .map(render_card_html)
        .collect::<Vec<_>>()
        .join("\n");

    let html = render_page(
        "CTF App",
        &format!(
            "<header class=\"topbar\"><div><h1>Challenges</h1><p class=\"muted\">Logged in as {}</p></div><form method=\"post\" action=\"/logout\"><button class=\"btn secondary\" type=\"submit\">Logout</button></form></header><div class=\"grid\">{}</div>",
            escape_html(&user_label),
            cards_html
        ),
    );

    Html(html).into_response()
}

pub async fn spawn_instance(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path((ctf_name, challenge_name)): Path<(String, String)>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    if db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .unwrap_or(false)
    {
        return Redirect::to(&format!(
            "{}?message={}",
            challenge_route(&ctf_name, &challenge_name),
            urlencoding::encode("challenge already solved")
        ))
        .into_response();
    }

    let instance_name = format!(
        "{}-{}-{}",
        state.config.instance_name_prefix,
        sanitize_k8s_name(&challenge_name),
        &Uuid::new_v4().to_string()[..8]
    )
    .chars()
    .take(63)
    .collect::<String>();

    let mut instance = Instance::new(
        &instance_name,
        InstanceSpec {
            ctf: ctf_name.clone(),
            challenge: challenge_name.clone(),
            conditions: Default::default(),
        },
    );

    let user_label = user
        .name
        .clone()
        .or(user.email.clone())
        .unwrap_or_else(|| user.sub.clone());
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    let Some(challenge_namespace) =
        db::find_challenge_namespace(&state.db, &challenge_name, &ctf_name)
            .await
            .ok()
            .flatten()
    else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            "challenge not found in watched namespaces",
        )
            .into_response();
    };

    let duration_seconds =
        resolve_instance_duration_seconds(&state, &challenge_namespace, &ctf_name, &challenge_name)
            .await;

    let api: Api<Instance> = Api::namespaced(state.kube.clone(), &challenge_namespace);

    let annotations = instance
        .metadata
        .annotations
        .get_or_insert_with(BTreeMap::new);
    annotations.insert("app.ctf.rs/user-id".to_string(), user.id.to_string());
    annotations.insert("app.ctf.rs/username".to_string(), user_label);
    annotations.insert("app.ctf.rs/spawned-at".to_string(), now_unix.to_string());
    annotations.insert(
        "app.ctf.rs/duration-seconds".to_string(),
        duration_seconds.to_string(),
    );
    annotations.insert(
        "app.ctf.rs/expires-at".to_string(),
        now_unix.saturating_add(duration_seconds).to_string(),
    );

    if let Err(err) = api
        .patch(
            &instance_name,
            &PatchParams::apply("ctf-app").force(),
            &Patch::Apply(&instance),
        )
        .await
    {
        warn!(error = %err, "failed to spawn instance");
        return (
            axum::http::StatusCode::BAD_GATEWAY,
            "failed to create instance",
        )
            .into_response();
    }

    if let Err(err) = db::set_instance(
        &state.db,
        user.id,
        &challenge_name,
        &ctf_name,
        &challenge_namespace,
        &instance_name,
    )
    .await
    {
        warn!(error = %err, "failed to persist instance relation");
    }

    Redirect::to(&challenge_route(&ctf_name, &challenge_name)).into_response()
}

pub async fn submit_flag(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path((ctf_name, challenge_name)): Path<(String, String)>,
    Form(form): Form<SubmitFlagForm>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    if db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .unwrap_or(false)
    {
        return Redirect::to(&format!(
            "{}?submit=bad&flag={}&message={}",
            challenge_route(&ctf_name, &challenge_name),
            urlencoding::encode(&form.flag_name),
            urlencoding::encode("challenge already solved")
        ))
        .into_response();
    }

    let submitted_flag = form.submitted_flag.trim();
    let challenge_namespace = db::find_challenge_namespace(&state.db, &challenge_name, &ctf_name)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".to_string());

    let redirect_with = |submit: &str, message: &str| {
        Redirect::to(&format!(
            "{}?submit={}&flag={}&message={}",
            challenge_route(&ctf_name, &challenge_name),
            submit,
            urlencoding::encode(&form.flag_name),
            urlencoding::encode(message)
        ))
    };

    let Some(instance) = db::get_instance_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .ok()
        .flatten()
    else {
        if let Err(err) = persist_flag_submission(
            &state,
            user.id,
            &challenge_name,
            &ctf_name,
            &challenge_namespace,
            &form.flag_name,
            submitted_flag,
            false,
        )
        .await
        {
            warn!(error = %err, "failed to persist flag submission");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "failed to persist flag submission",
            )
                .into_response();
        }
        return redirect_with("bad", "spawn an instance first").into_response();
    };

    let api: Api<Instance> = Api::namespaced(state.kube.clone(), &instance.namespace);
    let Ok(instance_obj) = api.get(&instance.instance_name).await else {
        if let Err(err) = persist_flag_submission(
            &state,
            user.id,
            &challenge_name,
            &ctf_name,
            &instance.namespace,
            &form.flag_name,
            submitted_flag,
            false,
        )
        .await
        {
            warn!(error = %err, "failed to persist flag submission");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "failed to persist flag submission",
            )
                .into_response();
        }
        return redirect_with("bad", "instance not found").into_response();
    };

    if !is_instance_owned_by_user(&instance_obj, &user) {
        if let Err(err) = persist_flag_submission(
            &state,
            user.id,
            &challenge_name,
            &ctf_name,
            &instance.namespace,
            &form.flag_name,
            submitted_flag,
            false,
        )
        .await
        {
            warn!(error = %err, "failed to persist flag submission");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "failed to persist flag submission",
            )
                .into_response();
        }
        return redirect_with("bad", "instance belongs to a different user").into_response();
    }

    let Some(uid) = instance_obj.metadata.uid.clone() else {
        if let Err(err) = persist_flag_submission(
            &state,
            user.id,
            &challenge_name,
            &ctf_name,
            &instance.namespace,
            &form.flag_name,
            submitted_flag,
            false,
        )
        .await
        {
            warn!(error = %err, "failed to persist flag submission");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "failed to persist flag submission",
            )
                .into_response();
        }
        return redirect_with("bad", "instance is not ready yet").into_response();
    };

    let runtime_ns = format!("ctf-inst-{uid}");
    let flag_name = format!("{}-{}", instance.instance_name, form.flag_name);
    let flag_api: Api<Flag> = Api::namespaced(state.kube.clone(), &runtime_ns);

    let accepted = match flag_api.get(&flag_name).await {
        Ok(flag_obj) => {
            let core_flag = CoreFlag {
                prefix: flag_obj.spec.prefix,
                format: flag_obj.spec.format,
                body: flag_obj.spec.body,
                random: flag_obj.spec.random,
                options: flag_obj.spec.options,
            };
            core_flag.verify(submitted_flag)
        }
        Err(_) => false,
    };

    if let Err(err) = persist_flag_submission(
        &state,
        user.id,
        &challenge_name,
        &ctf_name,
        &instance.namespace,
        &form.flag_name,
        submitted_flag,
        accepted,
    )
    .await
    {
        warn!(error = %err, "failed to persist flag submission");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "failed to persist flag submission",
        )
            .into_response();
    }

    if accepted {
        if let Err(err) = db::mark_challenge_solved(
            &state.db,
            user.id,
            &instance.namespace,
            &challenge_name,
            &ctf_name,
        )
        .await
        {
            warn!(error = %err, "failed to persist solved challenge state");
        }

        let api: Api<Instance> = Api::namespaced(state.kube.clone(), &instance.namespace);
        match api
            .delete(&instance.instance_name, &kube::api::DeleteParams::default())
            .await
        {
            Ok(_) => {}
            Err(kube::Error::Api(ae)) if ae.code == 404 => {}
            Err(err) => warn!(error = %err, "failed to delete solved instance"),
        }

        if let Err(err) =
            db::clear_instance_for_user(&state.db, user.id, &challenge_name, &ctf_name).await
        {
            warn!(error = %err, "failed to clear solved instance relation");
        }

        return redirect_with("good", "challenge solved").into_response();
    }

    redirect_with(
        if accepted { "good" } else { "bad" },
        if accepted {
            "flag accepted"
        } else {
            "flag is not correct"
        },
    )
    .into_response()
}

pub async fn extend_instance(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path((ctf_name, challenge_name)): Path<(String, String)>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    if db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .unwrap_or(false)
    {
        return Redirect::to(&format!(
            "{}?extend=bad&message={}",
            challenge_route(&ctf_name, &challenge_name),
            urlencoding::encode("challenge already solved")
        ))
        .into_response();
    }

    let redirect_with = |extend: &str, message: &str| {
        Redirect::to(&format!(
            "{}?extend={}&message={}",
            challenge_route(&ctf_name, &challenge_name),
            extend,
            urlencoding::encode(message)
        ))
    };

    let Some(instance) = db::get_instance_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .ok()
        .flatten()
    else {
        return redirect_with("bad", "spawn an instance first").into_response();
    };

    let api: Api<Instance> = Api::namespaced(state.kube.clone(), &instance.namespace);
    let Ok(instance_obj) = api.get(&instance.instance_name).await else {
        return redirect_with("bad", "instance not found").into_response();
    };

    if !is_instance_owned_by_user(&instance_obj, &user) {
        return redirect_with("bad", "instance belongs to a different user").into_response();
    }

    let duration_seconds =
        resolve_instance_duration_seconds(&state, &instance.namespace, &ctf_name, &challenge_name)
            .await;

    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    let current_expires_at = instance_obj
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get("app.ctf.rs/expires-at"))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| now_unix.saturating_add(duration_seconds));

    let time_left_seconds = current_expires_at.saturating_sub(now_unix);
    if time_left_seconds > 300 {
        return redirect_with("bad", "extension is available only in the last 5 minutes")
            .into_response();
    }

    let new_expires_at = current_expires_at
        .max(now_unix)
        .saturating_add(duration_seconds);

    let mut annotations = instance_obj
        .metadata
        .annotations
        .clone()
        .unwrap_or_default();
    annotations.insert(
        "app.ctf.rs/duration-seconds".to_string(),
        duration_seconds.to_string(),
    );
    annotations.insert(
        "app.ctf.rs/expires-at".to_string(),
        new_expires_at.to_string(),
    );

    let patch = serde_json::json!({
        "metadata": {
            "annotations": annotations,
        }
    });

    if let Err(err) = api
        .patch(
            &instance.instance_name,
            &PatchParams::apply("ctf-app"),
            &Patch::Merge(&patch),
        )
        .await
    {
        warn!(error = %err, "failed to extend instance ttl");
        return redirect_with("bad", "failed to extend instance").into_response();
    }

    redirect_with("ok", "instance duration extended").into_response()
}

pub async fn instance_state(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path((ctf_name, challenge_name)): Path<(String, String)>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    let Some(challenge) = db::get_challenge(&state.db, &ctf_name, &challenge_name)
        .await
        .ok()
        .flatten()
    else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(InstanceStateResponse {
                has_instance: false,
                ready: false,
                can_extend: false,
                solved: false,
                time_left_seconds: None,
                instance_name: None,
            }),
        )
            .into_response();
    };

    if !has_ctf_access(&user, &challenge) {
        return (axum::http::StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let solved = db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .unwrap_or(false);

    if solved {
        return Json(InstanceStateResponse {
            has_instance: false,
            ready: false,
            can_extend: false,
            solved: true,
            time_left_seconds: None,
            instance_name: None,
        })
        .into_response();
    }

    let Some(instance) = db::get_instance_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .ok()
        .flatten()
    else {
        return Json(InstanceStateResponse {
            has_instance: false,
            ready: false,
            can_extend: false,
            solved: false,
            time_left_seconds: None,
            instance_name: None,
        })
        .into_response();
    };

    let api: Api<Instance> = Api::namespaced(state.kube.clone(), &instance.namespace);
    let Ok(instance_obj) = api.get(&instance.instance_name).await else {
        return Json(InstanceStateResponse {
            has_instance: false,
            ready: false,
            can_extend: false,
            solved: false,
            time_left_seconds: None,
            instance_name: None,
        })
        .into_response();
    };

    if !is_instance_owned_by_user(&instance_obj, &user) {
        return Json(InstanceStateResponse {
            has_instance: false,
            ready: false,
            can_extend: false,
            solved: false,
            time_left_seconds: None,
            instance_name: None,
        })
        .into_response();
    }

    let ready = instance_ready_from_resource(&instance_obj);
    let time_left_seconds = instance_time_left_seconds(&instance_obj);
    let can_extend = ready
        && time_left_seconds
            .map(|seconds| seconds <= 300 && seconds > 0)
            .unwrap_or(false);

    Json(InstanceStateResponse {
        has_instance: true,
        ready,
        can_extend,
        solved: false,
        time_left_seconds,
        instance_name: Some(instance.instance_name),
    })
    .into_response()
}

pub async fn download_file(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path((ctf_name, challenge_name, file_idx)): Path<(String, String, usize)>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    let Some(challenge) = db::get_challenge(&state.db, &ctf_name, &challenge_name)
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

pub async fn challenge_page(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path((ctf_name, challenge_name)): Path<(String, String)>,
    Query(query): Query<ChallengePageQuery>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    let Some(challenge) = db::get_challenge(&state.db, &ctf_name, &challenge_name)
        .await
        .ok()
        .flatten()
    else {
        return (axum::http::StatusCode::NOT_FOUND, "challenge not found").into_response();
    };

    if !has_ctf_access(&user, &challenge) {
        return (axum::http::StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let solved = db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .unwrap_or(false);

    let mut instance = db::get_instance_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .ok()
        .flatten();

    let mut ready = false;
    let mut time_left_seconds: Option<u64> = None;
    let mut can_extend = false;
    if let Some(existing) = &instance {
        let api: Api<Instance> = Api::namespaced(state.kube.clone(), &existing.namespace);
        match api.get(&existing.instance_name).await {
            Ok(instance_obj) if is_instance_owned_by_user(&instance_obj, &user) => {
                ready = instance_ready_from_resource(&instance_obj);
                time_left_seconds = instance_time_left_seconds(&instance_obj);
                can_extend = ready
                    && time_left_seconds
                        .map(|seconds| seconds <= 300 && seconds > 0)
                        .unwrap_or(false);
            }
            Ok(_) => {
                instance = None;
                ready = false;
                time_left_seconds = None;
                can_extend = false;
            }
            Err(_) => {
                ready = false;
                time_left_seconds = None;
                can_extend = false;
            }
        }
    }

    let user_label = user
        .name
        .clone()
        .or(user.email.clone())
        .unwrap_or_else(|| user.sub.clone());

    let submit_feedback = submission_feedback(
        query.submit.as_deref(),
        query.flag.as_deref(),
        query.message.as_deref(),
    );
    let extend_feedback = extend_feedback(query.extend.as_deref(), query.message.as_deref());
    let card_html = render_challenge_detail_html(
        &challenge,
        instance.as_ref(),
        ready,
        time_left_seconds,
        can_extend,
        solved,
        submit_feedback.as_ref(),
        extend_feedback.as_deref(),
    );

    let live_script = render_instance_live_script(&ctf_name, &challenge_name);

    let html = render_page(
        "CTF App",
        &format!(
            "<header class=\"topbar\"><div><h1>{}</h1><p class=\"muted\">Logged in as {}</p></div><div><a class=\"btn secondary\" href=\"/\">Back</a><form method=\"post\" action=\"/logout\" style=\"display:inline-block;margin-left:.5rem;\"><button class=\"btn secondary\" type=\"submit\">Logout</button></form></div></header><div class=\"grid\">{}</div>{}",
            escape_html(&challenge.full_name),
            escape_html(&user_label),
            card_html,
            live_script
        ),
    );

    Html(html).into_response()
}

async fn current_user(state: &Arc<AppState>, jar: &CookieJar) -> anyhow::Result<Option<User>> {
    let Some(session_cookie) = jar.get(&state.config.session_cookie_name) else {
        return Ok(None);
    };

    db::get_user_by_session(&state.db, session_cookie.value()).await
}

async fn persist_flag_submission(
    state: &Arc<AppState>,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    challenge_namespace: &str,
    flag_name: &str,
    submitted_flag: &str,
    accepted: bool,
) -> anyhow::Result<()> {
    db::add_flag_submission(
        &state.db,
        user_id,
        challenge_name,
        ctf_name,
        challenge_namespace,
        flag_name,
        submitted_flag,
        accepted,
    )
    .await
}

fn instance_ready_from_resource(instance: &Instance) -> bool {
    let Some(status) = instance.status.as_ref() else {
        return false;
    };

    let Ok(status_json) = serde_json::to_value(status) else {
        return false;
    };
    let Some(conditions) = status_json.get("conditions").and_then(|c| c.as_array()) else {
        return false;
    };

    conditions.iter().any(|cond| {
        cond.get("type").and_then(|v| v.as_str()) == Some("Ready")
            && cond.get("status").and_then(|v| v.as_str()) == Some("True")
    })
}

fn instance_time_left_seconds(instance: &Instance) -> Option<u64> {
    let now_unix = now_unix_seconds();
    instance
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get("app.ctf.rs/expires-at"))
        .and_then(|value| value.parse::<u64>().ok())
        .map(|expires_at| expires_at.saturating_sub(now_unix))
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn is_instance_owned_by_user(instance: &Instance, user: &User) -> bool {
    instance
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get("app.ctf.rs/user-id"))
        .map(|value| value == &user.id.to_string())
        .unwrap_or(false)
}

fn sanitize_k8s_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "challenge".to_string()
    } else {
        out
    }
}

#[derive(Clone)]
struct ChallengeCardView {
    challenge: ChallengeView,
    solved: bool,
}

fn render_card_html(card: &ChallengeCardView) -> String {
    let challenge = &card.challenge;
    let mut out = String::new();

    out.push_str(&format!(
        "<article class=\"tile{}\">",
        if card.solved { " solved" } else { "" }
    ));
    out.push_str(&format!("<h2>{}</h2>", escape_html(&challenge.full_name)));
    if card.solved {
        out.push_str("<p class=\"solved-badge\">Solved</p>");
    }
    out.push_str(&format!(
        "<p class=\"muted\">{}</p>",
        escape_html(challenge.description.as_deref().unwrap_or("No description"))
    ));
    out.push_str(&format!(
        "<div class=\"meta\">CTF: {}</div>",
        escape_html(&challenge.ctf_name)
    ));

    out.push_str("<h3>Files</h3><ul>");
    for (index, file) in challenge.files.iter().enumerate() {
        let file_link = file_route(&challenge.ctf_name, &challenge.name, index);
        let file_label = display_file_name(file);
        out.push_str(&format!(
            "<li><a href=\"{}\" target=\"_blank\">{}</a></li>",
            escape_html(&file_link),
            escape_html(&file_label)
        ));
    }
    out.push_str("</ul>");

    out.push_str(&format!(
        "<a class=\"btn\" href=\"{}\">Open challenge</a>",
        challenge_route(&challenge.ctf_name, &challenge.name)
    ));

    out.push_str("</article>");
    out
}

fn render_challenge_detail_html(
    challenge: &ChallengeView,
    instance: Option<&db::InstanceView>,
    ready: bool,
    time_left_seconds: Option<u64>,
    can_extend: bool,
    solved: bool,
    submit_feedback: Option<&SubmissionFeedback>,
    extend_feedback: Option<&str>,
) -> String {
    let mut out = String::new();
    let base = challenge_route(&challenge.ctf_name, &challenge.name);
    let spawn_action = format!("{base}spawn");
    let extend_action = format!("{base}extend");
    let submit_action = format!("{base}submit");

    out.push_str(&format!(
        "<article class=\"tile{}\">",
        if solved { " solved" } else { "" }
    ));
    out.push_str(&format!("<h2>{}</h2>", escape_html(&challenge.full_name)));
    if solved {
        out.push_str("<p class=\"solved-badge\">Challenge solved</p>");
    }
    out.push_str(&format!(
        "<p class=\"muted\">{}</p>",
        escape_html(challenge.description.as_deref().unwrap_or("No description"))
    ));
    out.push_str(&format!(
        "<div class=\"meta\">CTF: {}</div>",
        escape_html(&challenge.ctf_name)
    ));

    out.push_str("<h3>Files</h3><ul>");
    for (index, file) in challenge.files.iter().enumerate() {
        let file_link = file_route(&challenge.ctf_name, &challenge.name, index);
        let file_label = display_file_name(file);
        out.push_str(&format!(
            "<li><a href=\"{}\" target=\"_blank\">{}</a></li>",
            escape_html(&file_link),
            escape_html(&file_label)
        ));
    }
    out.push_str("</ul>");

    let instance_status_text = if solved {
        "This challenge is solved. Instance is stopped and submissions are disabled.".to_string()
    } else {
        match (instance, ready, time_left_seconds) {
            (None, _, _) => "No active instance. Spawn to start the challenge.".to_string(),
            (Some(_), false, Some(0)) => {
                "Instance expired. Spawn a new one to continue.".to_string()
            }
            (Some(_), false, _) => {
                "Instance is starting. This page updates automatically when it is ready."
                    .to_string()
            }
            (Some(_), true, _) => "Instance is ready.".to_string(),
        }
    };

    out.push_str(&format!(
        "<p id=\"instance-status\" class=\"muted\">{}</p>",
        escape_html(&instance_status_text)
    ));

    out.push_str(&format!(
        "<p id=\"instance-timer\" class=\"muted\">{}</p>",
        escape_html(
            &time_left_seconds
                .map(format_countdown)
                .map(|value| format!("Time left: {value}"))
                .unwrap_or_default()
        )
    ));

    if let Some(message) = extend_feedback {
        out.push_str(&format!("<p>{}</p>", escape_html(message)));
    }

    if solved {
    } else {
        match (instance, ready) {
            (None, _) => {
                out.push_str(&format!(
                    "<form method=\"post\" action=\"{}\"><button class=\"btn\" type=\"submit\">Spawn</button></form>",
                    escape_html(&spawn_action)
                ));
            }
            (Some(_), false) => {
                out.push_str(&format!(
                    "<form method=\"post\" action=\"{}\"><button class=\"btn\" type=\"submit\">Spawn again</button></form>",
                    escape_html(&spawn_action)
                ));
                out.push_str(&render_extend_form(&extend_action, can_extend));
            }
            (Some(_), true) => {
                out.push_str(&render_extend_form(&extend_action, can_extend));
                out.push_str("<h3>Submit flags</h3>");
                for flag_name in &challenge.flags {
                    out.push_str(&format!(
                        "<form method=\"post\" action=\"{}\" class=\"flag-form\">",
                        escape_html(&submit_action)
                    ));
                    out.push_str(&format!(
                        "<input type=\"hidden\" name=\"flag_name\" value=\"{}\" />",
                        escape_html(flag_name)
                    ));
                    out.push_str(&format!(
                        "<input type=\"text\" name=\"submitted_flag\" placeholder=\"{} flag\" required />",
                        escape_html(flag_name)
                    ));
                    out.push_str(
                        "<button class=\"btn\" type=\"submit\">Submit</button></form>",
                    );
                    if let Some(feedback) =
                        submit_feedback.filter(|feedback| feedback.flag_name == *flag_name)
                    {
                        out.push_str(&render_submission_feedback_for_flag(feedback));
                    }
                }
            }
        }
    }

    out.push_str("</article>");
    out
}

struct SubmissionFeedback {
    kind: String,
    flag_name: String,
    message: String,
}

fn submission_feedback(
    submit: Option<&str>,
    flag: Option<&str>,
    message: Option<&str>,
) -> Option<SubmissionFeedback> {
    let Some(result) = submit else {
        return None;
    };

    let kind = match result {
        "good" | "bad" => result,
        _ => return None,
    };

    Some(SubmissionFeedback {
        kind: kind.to_string(),
        flag_name: flag.unwrap_or("flag").to_string(),
        message: message.unwrap_or_default().to_string(),
    })
}

fn render_submission_feedback_for_flag(feedback: &SubmissionFeedback) -> String {
    match feedback.kind.as_str() {
        "good" => format!(
            "<p><strong>Good flag:</strong> {} accepted. {}</p>",
            escape_html(&feedback.flag_name),
            escape_html(&feedback.message)
        ),
        "bad" => format!(
            "<p><strong>Bad flag:</strong> {}. {}</p>",
            escape_html(&feedback.flag_name),
            escape_html(&feedback.message)
        ),
        _ => String::new(),
    }
}

fn render_extend_form(extend_action: &str, visible: bool) -> String {
    format!(
        "<form id=\"extend-form\" method=\"post\" action=\"{}\" style=\"display:{}\"><button class=\"btn secondary\" type=\"submit\">Extend</button></form>",
        escape_html(extend_action),
        if visible { "block" } else { "none" }
    )
}

fn format_countdown(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

fn render_instance_live_script(ctf_name: &str, challenge_name: &str) -> String {
    let endpoint = format!(
        "/ctf/{}/challenge/{}/instance-state",
        urlencoding::encode(ctf_name),
        urlencoding::encode(challenge_name)
    );

    format!(
        "<script>(function(){{const endpoint='{endpoint}';const statusEl=document.getElementById('instance-status');const timerEl=document.getElementById('instance-timer');const extendEl=document.getElementById('extend-form');let lastState=null;let localSeconds=null;function fmt(s){{const h=Math.floor(s/3600);const m=Math.floor((s%3600)/60);const sec=s%60;if(h>0) return `${{h}}h ${{m}}m ${{sec}}s`;if(m>0) return `${{m}}m ${{sec}}s`;return `${{sec}}s`;}}function setStatus(state){{if(!statusEl) return;if(state.solved){{statusEl.textContent='This challenge is solved. Instance is stopped and submissions are disabled.';return;}}if(!state.has_instance){{statusEl.textContent='No active instance. Spawn to start the challenge.';return;}}if(!state.ready){{if(typeof state.time_left_seconds==='number'&&state.time_left_seconds===0){{statusEl.textContent='Instance expired. Spawn a new one to continue.';}}else{{statusEl.textContent='Instance is starting. This page updates automatically when it is ready.';}}return;}}statusEl.textContent='Instance is ready.';}}function tick(){{if(localSeconds===null) return;localSeconds=Math.max(0,localSeconds-1);if(timerEl) timerEl.textContent='Time left: '+fmt(localSeconds);if(extendEl) extendEl.style.display=(localSeconds<=300 && localSeconds>0)?'block':'none';}}async function poll(){{try{{const res=await fetch(endpoint,{{credentials:'same-origin',cache:'no-store'}});if(!res.ok) return;const state=await res.json();setStatus(state);const key=`${{state.solved}}|${{state.has_instance}}|${{state.ready}}|${{state.instance_name||''}}`;if(lastState!==null&&key!==lastState){{window.location.reload();return;}}lastState=key;if(state.solved){{localSeconds=null;if(timerEl) timerEl.textContent='';if(extendEl) extendEl.style.display='none';return;}}if(!state.has_instance){{localSeconds=null;if(timerEl) timerEl.textContent='';if(extendEl) extendEl.style.display='none';return;}}if(typeof state.time_left_seconds==='number'){{localSeconds=state.time_left_seconds;if(timerEl) timerEl.textContent='Time left: '+fmt(localSeconds);}}else{{localSeconds=null;if(timerEl) timerEl.textContent='';}}if(extendEl) extendEl.style.display=state.can_extend?'block':'none';}}catch(_err){{}}}}setInterval(tick,1000);setInterval(poll,5000);poll();}})();</script>",
    )
}

fn extend_feedback(extend: Option<&str>, message: Option<&str>) -> Option<String> {
    match extend {
        Some("ok") => Some(message.unwrap_or("instance extended").to_string()),
        Some("bad") => Some(message.unwrap_or("failed to extend instance").to_string()),
        _ => None,
    }
}

async fn resolve_instance_duration_seconds(
    state: &Arc<AppState>,
    namespace: &str,
    ctf_name: &str,
    challenge_name: &str,
) -> u64 {
    let ctf_api: Api<Ctf> = Api::namespaced(state.kube.clone(), namespace);
    let challenge_api: Api<Challenge> = Api::namespaced(state.kube.clone(), namespace);

    let ctf_duration = ctf_api
        .get(ctf_name)
        .await
        .ok()
        .map(|ctf| u64::from(ctf.spec.instance_duration_seconds))
        .unwrap_or(15 * 60);

    challenge_api
        .get(challenge_name)
        .await
        .ok()
        .and_then(|challenge| challenge.spec.instance_duration_seconds.map(u64::from))
        .unwrap_or(ctf_duration)
}

fn challenge_route(ctf_name: &str, challenge_name: &str) -> String {
    format!(
        "/ctf/{}/challenge/{}/",
        urlencoding::encode(ctf_name),
        urlencoding::encode(challenge_name)
    )
}

fn file_route(ctf_name: &str, challenge_name: &str, file_idx: usize) -> String {
    format!(
        "/file/{}/{}/{}",
        urlencoding::encode(ctf_name),
        urlencoding::encode(challenge_name),
        file_idx
    )
}

fn parse_s3_bucket_key(
    file_url: &str,
    configured_endpoint: Option<&str>,
) -> Option<(String, String)> {
    let parsed = Url::parse(file_url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let path = parsed.path().trim_start_matches('/');

    if let Some(endpoint) = configured_endpoint {
        let endpoint = endpoint.trim().trim_end_matches('/');
        let endpoint_host = Url::parse(endpoint)
            .ok()
            .and_then(|url| url.host_str().map(|v| v.to_ascii_lowercase()))
            .or_else(|| {
                endpoint
                    .strip_prefix("https://")
                    .or_else(|| endpoint.strip_prefix("http://"))
                    .map(|value| value.to_ascii_lowercase())
            })?;

        if host == endpoint_host {
            let mut parts = path.splitn(2, '/');
            let bucket = parts.next()?.trim();
            let key = parts.next()?.trim();
            if bucket.is_empty() || key.is_empty() {
                return None;
            }
            return Some((bucket.to_string(), key.to_string()));
        }

        let suffix = format!(".{endpoint_host}");
        if host.ends_with(&suffix) {
            let bucket = host.trim_end_matches(&suffix).trim();
            if !bucket.is_empty() && !path.is_empty() {
                return Some((bucket.to_string(), path.to_string()));
            }
        }
    }

    if host == "s3.amazonaws.com" {
        let mut parts = path.splitn(2, '/');
        let bucket = parts.next()?.trim();
        let key = parts.next()?.trim();
        if bucket.is_empty() || key.is_empty() {
            return None;
        }
        return Some((bucket.to_string(), key.to_string()));
    }

    if let Some(bucket) = host.strip_suffix(".s3.amazonaws.com") {
        if bucket.is_empty() || path.is_empty() {
            return None;
        }
        return Some((bucket.to_string(), path.to_string()));
    }

    None
}

fn display_file_name(file_url: &str) -> String {
    if let Ok(parsed) = Url::parse(file_url) {
        if let Some(name) = parsed
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|name| !name.is_empty())
        {
            return name.to_string();
        }
    }

    file_url
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(file_url)
        .to_string()
}

fn has_ctf_access(user: &User, challenge: &ChallengeView) -> bool {
    if challenge.ctf_public {
        return true;
    }

    let namespace = challenge.namespace.to_ascii_lowercase();
    let ctf_name = challenge.ctf_name.to_ascii_lowercase();

    let required_roles = [
        format!("ctf:{namespace}:{ctf_name}:player"),
        format!("ctf:{namespace}:{ctf_name}:admin"),
        "ctf:admin".to_string(),
    ];

    user.roles.iter().any(|role| {
        required_roles
            .iter()
            .any(|required| role.eq_ignore_ascii_case(required))
    })
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_page(title: &str, body_html: &str) -> String {
    const STYLE: &str = include_str!("style.css");
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\" /><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" /><title>{}</title><style>{}</style></head><body><main class=\"container\">{}</main></body></html>",
        escape_html(title),
        STYLE,
        body_html
    )
}
