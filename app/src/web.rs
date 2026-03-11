use std::sync::Arc;

use axum::{
    extract::{Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use ctf_cli::k8s::{Flag, Instance, InstanceSpec};
use kube::{api::PatchParams, Api};
use serde::Deserialize;
use tracing::warn;
use uuid::Uuid;

use crate::{
    db::{self, ChallengeView, User},
    oauth,
    AppState,
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

pub async fn healthz() -> impl IntoResponse {
    "ok"
}

pub async fn login(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let Ok((auth_url, state_token, nonce)) = oauth::begin_login(&state.oidc_client) else {
        return Redirect::to("/");
    };

    state.pending_oauth.write().await.insert(
        state_token,
        oauth::PendingAuth {
            nonce,
        },
    );

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

    let claims = match oauth::finish_login(
        &state.oidc_client,
        query.code,
    )
    .await
    {
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
        .map(|challenge| ChallengeCardView { challenge })
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

    let instance_name = format!(
        "{}-{}-{}",
        state.config.instance_name_prefix,
        sanitize_k8s_name(&challenge_name),
        &Uuid::new_v4().to_string()[..8]
    )
    .chars()
    .take(63)
    .collect::<String>();

    let instance = Instance::new(
        &instance_name,
        InstanceSpec {
            ctf: ctf_name.clone(),
            challenge: challenge_name.clone(),
            conditions: Default::default(),
        },
    );

    let Some(challenge_namespace) = db::find_challenge_namespace(
        &state.db,
        &challenge_name,
        &ctf_name,
    )
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

    let api: Api<Instance> = Api::namespaced(state.kube.clone(), &challenge_namespace);
    if let Err(err) = api
        .patch(
            &instance_name,
            &PatchParams::apply("ctf-app").force(),
            &kube::api::Patch::Apply(&instance),
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

    let Some(instance) = db::get_instance_for_user(
        &state.db,
        user.id,
        &challenge_name,
        &ctf_name,
    )
    .await
    .ok()
    .flatten()
    else {
        return Redirect::to("/").into_response();
    };

    let api: Api<Instance> = Api::namespaced(state.kube.clone(), &instance.namespace);
    let Ok(instance_obj) = api.get(&instance.instance_name).await else {
        return Redirect::to("/").into_response();
    };

    let Some(uid) = instance_obj.metadata.uid.clone() else {
        return Redirect::to("/").into_response();
    };

    let runtime_ns = format!("ctf-inst-{uid}");
    let flag_name = format!("{}-{}", instance.instance_name, form.flag_name);
    let flag_api: Api<Flag> = Api::namespaced(state.kube.clone(), &runtime_ns);

    let accepted = match flag_api.get(&flag_name).await {
        Ok(flag_obj) => {
            let expected = flag_obj.status.and_then(|status| status.flag).unwrap_or_default();
            expected == form.submitted_flag
        }
        Err(_) => false,
    };

    let _ = db::add_flag_submission(
        &state.db,
        user.id,
        &challenge_name,
        &ctf_name,
        &instance.namespace,
        &form.flag_name,
        &form.submitted_flag,
        accepted,
    )
    .await;

    Redirect::to(&challenge_route(&ctf_name, &challenge_name)).into_response()
}

pub async fn challenge_page(
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
        return (axum::http::StatusCode::NOT_FOUND, "challenge not found").into_response();
    };

    if !has_ctf_access(&user, &challenge) {
        return (axum::http::StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let instance = db::get_instance_for_user(&state.db, user.id, &challenge_name, &ctf_name)
        .await
        .ok()
        .flatten();

    let mut ready = false;
    if let Some(existing) = &instance {
        ready = is_instance_ready(&state, &existing.namespace, &existing.instance_name)
            .await
            .unwrap_or(false);
    }

    let user_label = user
        .name
        .clone()
        .or(user.email.clone())
        .unwrap_or_else(|| user.sub.clone());

    let card_html = render_challenge_detail_html(&challenge, instance.as_ref(), ready);

    let html = render_page(
        "CTF App",
        &format!(
            "<header class=\"topbar\"><div><h1>{}</h1><p class=\"muted\">Logged in as {}</p></div><div><a class=\"btn secondary\" href=\"/\">Back</a><form method=\"post\" action=\"/logout\" style=\"display:inline-block;margin-left:.5rem;\"><button class=\"btn secondary\" type=\"submit\">Logout</button></form></div></header><div class=\"grid\">{}</div>",
            escape_html(&challenge.full_name),
            escape_html(&user_label),
            card_html
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

async fn is_instance_ready(state: &Arc<AppState>, namespace: &str, name: &str) -> anyhow::Result<bool> {
    let api: Api<Instance> = Api::namespaced(state.kube.clone(), namespace);
    let instance = api.get(name).await?;
    let Some(status) = instance.status else {
        return Ok(false);
    };

    let status_json = serde_json::to_value(status)?;
    let Some(conditions) = status_json.get("conditions").and_then(|c| c.as_array()) else {
        return Ok(false);
    };

    Ok(conditions.iter().any(|cond| {
        cond.get("type").and_then(|v| v.as_str()) == Some("Ready")
            && cond.get("status").and_then(|v| v.as_str()) == Some("True")
    }))
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
}

fn render_card_html(card: &ChallengeCardView) -> String {
    let challenge = &card.challenge;
    let mut out = String::new();

    out.push_str("<article class=\"tile\">");
    out.push_str(&format!("<h2>{}</h2>", escape_html(&challenge.full_name)));
    out.push_str(&format!(
        "<p class=\"muted\">{}</p>",
        escape_html(challenge.description.as_deref().unwrap_or("No description"))
    ));
    out.push_str(&format!(
        "<div class=\"meta\">CTF: {}</div>",
        escape_html(&challenge.ctf_name)
    ));

    out.push_str("<h3>Files</h3><ul>");
    for file in &challenge.files {
        out.push_str(&format!(
            "<li><a href=\"{}\" target=\"_blank\">{}</a></li>",
            escape_html(file),
            escape_html(file)
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
) -> String {
    let mut out = String::new();
    let base = challenge_route(&challenge.ctf_name, &challenge.name);
    let spawn_action = format!("{base}spawn");
    let submit_action = format!("{base}submit");

    out.push_str("<article class=\"tile\">");
    out.push_str(&format!("<h2>{}</h2>", escape_html(&challenge.full_name)));
    out.push_str(&format!(
        "<p class=\"muted\">{}</p>",
        escape_html(challenge.description.as_deref().unwrap_or("No description"))
    ));
    out.push_str(&format!(
        "<div class=\"meta\">CTF: {}</div>",
        escape_html(&challenge.ctf_name)
    ));

    out.push_str("<h3>Files</h3><ul>");
    for file in &challenge.files {
        out.push_str(&format!(
            "<li><a href=\"{}\" target=\"_blank\">{}</a></li>",
            escape_html(file),
            escape_html(file)
        ));
    }
    out.push_str("</ul>");

    match (instance, ready) {
        (None, _) => {
            out.push_str(&format!(
                "<form method=\"post\" action=\"{}\"><button class=\"btn\" type=\"submit\">Spawn</button></form>",
                escape_html(&spawn_action)
            ));
        }
        (Some(_), false) => {
            out.push_str("<p class=\"muted\">Instance spawning... refresh in a few seconds.</p>");
            out.push_str(&format!(
                "<form method=\"post\" action=\"{}\"><button class=\"btn\" type=\"submit\">Spawn again</button></form>",
                escape_html(&spawn_action)
            ));
        }
        (Some(_), true) => {
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
                out.push_str("<button class=\"btn\" type=\"submit\">Submit</button></form>");
            }
        }
    }

    out.push_str("</article>");
    out
}

fn challenge_route(ctf_name: &str, challenge_name: &str) -> String {
    format!(
        "/ctf/{}/challenge/{}/",
        urlencoding::encode(ctf_name),
        urlencoding::encode(challenge_name)
    )
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

    user.roles
        .iter()
        .any(|role| required_roles.iter().any(|required| role.eq_ignore_ascii_case(required)))
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
