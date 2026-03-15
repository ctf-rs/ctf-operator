use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use ctf_cli::k8s::{Instance, InstanceSpec};
use kube::{
    Api, ResourceExt,
    api::{Patch, PatchParams},
};
use serde::Serialize;
use tracing::warn;

use crate::{
    db,
    helpers::{
        challenge_route, current_user, extract_client_ip, has_ctf_access, now_unix_seconds,
        sanitize_k8s_name,
    },
    k8s::{
        challenges,
        helpers::{
            instance_ready_from_resource, instance_time_left_seconds, is_instance_deleting,
            is_instance_owned_by_user, resolve_instance_duration_seconds,
        },
        instances as k8s_instances,
    },
    state::AppState,
};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct InstanceStateResponse {
    has_instance: bool,
    ready: bool,
    can_extend: bool,
    solved: bool,
    deleting: bool,
    time_left_seconds: Option<u64>,
    instance_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Spawn instance
// ---------------------------------------------------------------------------

pub async fn spawn_instance(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(challenge_name): Path<String>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    let ctf_name = &state.config.ctf_name;
    let client_ip = extract_client_ip(state.config.behind_reverse_proxy, &headers, Some(addr));

    if db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, ctf_name)
        .await
        .unwrap_or(false)
    {
        return Redirect::to(&format!(
            "{}?message={}",
            challenge_route(&challenge_name),
            urlencoding::encode("challenge already solved")
        ))
        .into_response();
    }

    let Some(challenge_namespace) = challenges::find_challenge_namespace(&state, &challenge_name)
    else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            "challenge not found in watched namespaces",
        )
            .into_response();
    };

    // ---- Guard: one instance per user per challenge -------------------
    // If the user already has a tracked instance in the Store, check whether
    // it is expired.  If expired, delete it and allow re-spawn.
    if let Some(existing) =
        k8s_instances::get_instance_for_user(&state, user.id, &challenge_name, ctf_name)
    {
        let time_left = instance_time_left_seconds(&existing);
        if time_left == Some(0) {
            // Instance expired → delete it and allow re-spawn
            let _ = k8s_instances::delete_instance(
                &state.kube,
                existing.metadata.namespace.as_deref().unwrap_or_default(),
                &existing.name_any(),
            )
            .await;
        } else {
            // Instance still alive and not expired → redirect back.
            return Redirect::to(&challenge_route(&challenge_name)).into_response();
        }
    }

    // ---- Deterministic name: prefix-challenge-userId ------------------
    let instance_name: String = format!(
        "{}-{}-u{}",
        state.config.instance_name_prefix,
        sanitize_k8s_name(&challenge_name),
        user.id,
    )
    .chars()
    .take(63)
    .collect();

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

    let duration_seconds =
        resolve_instance_duration_seconds(&state, &challenge_namespace, ctf_name, &challenge_name)
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

    // Audit log: instance spawned
    if let Err(err) = db::log_instance_action(
        &state.db,
        user.id,
        &instance_name,
        &challenge_name,
        ctf_name,
        "spawn",
        client_ip.as_deref(),
    )
    .await
    {
        warn!(error = %err, "failed to log instance spawn audit");
    }

    Redirect::to(&challenge_route(&challenge_name)).into_response()
}

// ---------------------------------------------------------------------------
// Extend instance
// ---------------------------------------------------------------------------

pub async fn extend_instance(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(challenge_name): Path<String>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    let ctf_name = &state.config.ctf_name;
    let client_ip = extract_client_ip(state.config.behind_reverse_proxy, &headers, Some(addr));

    if db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, ctf_name)
        .await
        .unwrap_or(false)
    {
        return Redirect::to(&format!(
            "{}?extend=bad&message={}",
            challenge_route(&challenge_name),
            urlencoding::encode("challenge already solved")
        ))
        .into_response();
    }

    let redirect_with = |extend: &str, message: &str| {
        Redirect::to(&format!(
            "{}?extend={}&message={}",
            challenge_route(&challenge_name),
            extend,
            urlencoding::encode(message)
        ))
    };

    let Some(instance) =
        k8s_instances::get_instance_for_user(&state, user.id, &challenge_name, ctf_name)
    else {
        return redirect_with("bad", "spawn an instance first").into_response();
    };

    if !is_instance_owned_by_user(&instance, &user) {
        return redirect_with("bad", "instance belongs to a different user").into_response();
    }

    let instance_ns = instance.metadata.namespace.clone().unwrap_or_default();
    let instance_name = instance.name_any();

    let duration_seconds =
        resolve_instance_duration_seconds(&state, &instance_ns, ctf_name, &challenge_name).await;

    let now_unix = now_unix_seconds();

    let current_expires_at = instance
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

    if let Err(err) = k8s_instances::extend_instance_ttl(
        &state.kube,
        &instance_ns,
        &instance_name,
        duration_seconds,
        new_expires_at,
    )
    .await
    {
        warn!(error = %err, "failed to extend instance ttl");
        return redirect_with("bad", "failed to extend instance").into_response();
    }

    // Audit log: instance extended
    if let Err(err) = db::log_instance_action(
        &state.db,
        user.id,
        &instance_name,
        &challenge_name,
        ctf_name,
        "extend",
        client_ip.as_deref(),
    )
    .await
    {
        warn!(error = %err, "failed to log instance extend audit");
    }

    redirect_with("ok", "instance duration extended").into_response()
}

// ---------------------------------------------------------------------------
// Instance state (JSON poll endpoint)
// ---------------------------------------------------------------------------

pub async fn instance_state(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(challenge_name): Path<String>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    let ctf_name = &state.config.ctf_name;

    let Some(challenge) = challenges::get_challenge(&state, &challenge_name) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(InstanceStateResponse {
                has_instance: false,
                ready: false,
                can_extend: false,
                solved: false,
                deleting: false,
                time_left_seconds: None,
                instance_name: None,
            }),
        )
            .into_response();
    };

    if !has_ctf_access(&user, &challenge) {
        return (axum::http::StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let solved = db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, ctf_name)
        .await
        .unwrap_or(false);

    if solved {
        return Json(InstanceStateResponse {
            has_instance: false,
            ready: false,
            can_extend: false,
            solved: true,
            deleting: false,
            time_left_seconds: None,
            instance_name: None,
        })
        .into_response();
    }

    let Some(instance) =
        k8s_instances::get_instance_for_user(&state, user.id, &challenge_name, ctf_name)
    else {
        return Json(InstanceStateResponse {
            has_instance: false,
            ready: false,
            can_extend: false,
            solved: false,
            deleting: false,
            time_left_seconds: None,
            instance_name: None,
        })
        .into_response();
    };

    if !is_instance_owned_by_user(&instance, &user) {
        return Json(InstanceStateResponse {
            has_instance: false,
            ready: false,
            can_extend: false,
            solved: false,
            deleting: false,
            time_left_seconds: None,
            instance_name: None,
        })
        .into_response();
    }

    let ready = instance_ready_from_resource(&instance);
    let time_left_seconds = instance_time_left_seconds(&instance);
    let deleting = is_instance_deleting(&instance);
    let can_extend = ready
        && !deleting
        && time_left_seconds
            .map(|seconds| seconds <= 300 && seconds > 0)
            .unwrap_or(false);

    Json(InstanceStateResponse {
        has_instance: true,
        ready,
        can_extend,
        solved: false,
        deleting,
        time_left_seconds,
        instance_name: Some(instance.name_any()),
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Stop (delete) instance
// ---------------------------------------------------------------------------

pub async fn stop_instance(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(challenge_name): Path<String>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    let ctf_name = &state.config.ctf_name;
    let client_ip = extract_client_ip(state.config.behind_reverse_proxy, &headers, Some(addr));

    let redirect_with = |kind: &str, message: &str| {
        Redirect::to(&format!(
            "{}?stop={}&message={}",
            challenge_route(&challenge_name),
            kind,
            urlencoding::encode(message)
        ))
    };

    let Some(instance) =
        k8s_instances::get_instance_for_user(&state, user.id, &challenge_name, ctf_name)
    else {
        return redirect_with("bad", "no instance to stop").into_response();
    };

    // Verify ownership before deleting
    if !is_instance_owned_by_user(&instance, &user) {
        return redirect_with("bad", "instance belongs to a different user").into_response();
    }

    let instance_name = instance.name_any();
    let instance_ns = instance.metadata.namespace.clone().unwrap_or_default();

    // Delete the instance from cluster (initiates termination)
    if let Err(err) =
        k8s_instances::delete_instance(&state.kube, &instance_ns, &instance_name).await
    {
        warn!(error = %err, "failed to stop instance");
        return redirect_with("bad", "failed to stop instance").into_response();
    }

    // Audit log: instance stopped
    if let Err(err) = db::log_instance_action(
        &state.db,
        user.id,
        &instance_name,
        &challenge_name,
        ctf_name,
        "stop",
        client_ip.as_deref(),
    )
    .await
    {
        warn!(error = %err, "failed to log instance stop audit");
    }

    redirect_with("ok", "instance stopping").into_response()
}
