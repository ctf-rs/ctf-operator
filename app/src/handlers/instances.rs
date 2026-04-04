use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
// Note: Extension + State + Path are still used by the manual `instance_state` handler.
use ctf_cli::k8s::{Instance, InstanceSpec, InstanceTimeLeft, InstanceUser};
use kube::ResourceExt;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    db,
    helpers::{challenge_route, extract_client_ip, has_ctf_access, sanitize_k8s_name},
    k8s::{
        challenges,
        helpers::{
            instance_expires_at_epoch, instance_ready_from_resource, instance_time_left_seconds,
            is_instance_deleting, is_instance_owned_by_user, resolve_instance_duration_seconds,
        },
        instances as k8s_instances,
    },
    metrics,
    state::AppState,
};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct InstanceStateResponse {
    has_instance: bool,
    ready: bool,
    can_extend: bool,
    solved: bool,
    deleting: bool,
    time_left_seconds: Option<u64>,
    expires_at_epoch: Option<u64>,
    instance_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Spawn instance (server function)
// ---------------------------------------------------------------------------

#[server(endpoint = "spawn_instance")]
pub async fn spawn_instance(challenge_name: String) -> Result<(), ServerFnError> {
    let state = expect_context::<Arc<AppState>>();
    let Some(user) = use_context::<db::User>() else {
        leptos_axum::redirect("/login");
        return Ok(());
    };

    let headers: HeaderMap = leptos_axum::extract()
        .await
        .map_err(|e| ServerFnError::new(format!("headers: {e}")))?;
    let socket_addr = leptos_axum::extract::<ConnectInfo<SocketAddr>>()
        .await
        .ok()
        .map(|ci| ci.0);

    let ctf_name = &state.config.ctf_name;
    let client_ip = extract_client_ip(state.config.behind_reverse_proxy, &headers, socket_addr);

    let Some(challenge_namespace) = challenges::find_challenge_namespace(&state, &challenge_name)
    else {
        return Err(ServerFnError::new(
            "challenge not found in watched namespaces",
        ));
    };

    let challenge = challenges::get_challenge(&state, &challenge_name);
    let instancing = challenge.as_ref().map(|c| c.instancing).unwrap_or(true);

    // Global challenges: instances are managed by the operator, not users
    if !instancing {
        leptos_axum::redirect(&challenge_route(&challenge_name));
        return Ok(());
    }

    // ---- Guard: instance already exists --------------------------------
    if k8s_instances::get_instance_for_user(&state, user.id, &challenge_name, ctf_name).is_some() {
        leptos_axum::redirect(&challenge_route(&challenge_name));
        return Ok(());
    }

    // ---- Guard: max instances per user ---------------------------------
    let user_instances = k8s_instances::list_instances_for_user(&state, user.id);
    if user_instances.len() >= state.config.max_instances_per_user {
        leptos_axum::redirect(&format!(
            "{}?message={}",
            challenge_route(&challenge_name),
            urlencoding::encode(&format!(
                "maximum number of instances ({}) reached, stop an existing instance first",
                state.config.max_instances_per_user
            ))
        ));
        return Ok(());
    }

    // ---- Deterministic name: prefix-ctf-challenge-userId --------------
    let instance_name: String = format!(
        "{}-{}-{}-u{}",
        state.config.instance_name_prefix,
        sanitize_k8s_name(ctf_name),
        sanitize_k8s_name(&challenge_name),
        user.id,
    )
    .chars()
    .take(63)
    .collect();

    let user_label = user
        .name
        .clone()
        .or(user.email.clone())
        .unwrap_or_else(|| user.sub.clone());

    let duration_seconds =
        resolve_instance_duration_seconds(&state, &challenge_namespace, ctf_name, &challenge_name)
            .await;

    let mut instance = Instance::new(
        &instance_name,
        InstanceSpec {
            ctf: ctf_name.clone(),
            challenge: challenge_name.clone(),
            user: Some(InstanceUser {
                id: user.id,
                username: user_label,
            }),
            duration_seconds: Some(duration_seconds),
        },
    );

    instance.metadata.namespace = Some(challenge_namespace.clone());

    if let Err(err) =
        k8s_instances::create_or_update_instance(&state.kube, &challenge_namespace, &instance).await
    {
        warn!(error = %err, "failed to spawn instance");
        return Err(ServerFnError::new("failed to create instance"));
    }

    // Audit log: instance spawned
    let m = metrics::get();
    m.instances_spawned
        .add(1, &m.attrs(&[metrics::challenge_kv(&challenge_name)]));
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

    leptos_axum::redirect(&challenge_route(&challenge_name));
    Ok(())
}

// ---------------------------------------------------------------------------
// Extend instance (server function)
// ---------------------------------------------------------------------------

#[server(endpoint = "extend_instance")]
pub async fn extend_instance(challenge_name: String) -> Result<(), ServerFnError> {
    let state = expect_context::<Arc<AppState>>();
    let Some(user) = use_context::<db::User>() else {
        leptos_axum::redirect("/login");
        return Ok(());
    };

    let headers: HeaderMap = leptos_axum::extract()
        .await
        .map_err(|e| ServerFnError::new(format!("headers: {e}")))?;
    let socket_addr = leptos_axum::extract::<ConnectInfo<SocketAddr>>()
        .await
        .ok()
        .map(|ci| ci.0);

    let ctf_name = &state.config.ctf_name;
    let client_ip = extract_client_ip(state.config.behind_reverse_proxy, &headers, socket_addr);

    // Global challenges cannot be extended by users
    let instancing = challenges::get_challenge(&state, &challenge_name)
        .map(|c| c.instancing)
        .unwrap_or(true);
    if !instancing {
        leptos_axum::redirect(&challenge_route(&challenge_name));
        return Ok(());
    }

    let redirect_with = |kind: &str, message: &str| {
        leptos_axum::redirect(&format!(
            "{}?extend={}&message={}",
            challenge_route(&challenge_name),
            kind,
            urlencoding::encode(message)
        ));
    };

    if db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, ctf_name)
        .await
        .unwrap_or(false)
    {
        redirect_with("bad", "challenge already solved");
        return Ok(());
    }

    let Some(instance) =
        k8s_instances::get_instance_for_challenge(&state, user.id, &challenge_name, ctf_name)
    else {
        redirect_with("bad", "spawn an instance first");
        return Ok(());
    };

    if !is_instance_owned_by_user(&instance, &user) {
        redirect_with("bad", "instance belongs to a different user");
        return Ok(());
    }

    let instance_ns = instance.metadata.namespace.clone().unwrap_or_default();
    let instance_name = instance.name_any();

    let extension_duration =
        resolve_instance_duration_seconds(&state, &instance_ns, ctf_name, &challenge_name).await;

    // Compute time left from ready_since + spec.duration_seconds instead of
    // status.expires_at, which lags behind spec patches and allows race
    // conditions when the button is clicked rapidly.
    let time_left_secs = match instance.computed_time_left() {
        Ok(InstanceTimeLeft::Remaining(dur)) => dur.as_secs(),
        Ok(InstanceTimeLeft::Expired) => 0,
        Err(err) => {
            warn!(error = %err, "cannot compute instance time left for extend guard");
            redirect_with("bad", "instance is not ready yet");
            return Ok(());
        }
    };

    if time_left_secs > 300 {
        redirect_with("bad", "extension is available only in the last 5 minutes");
        return Ok(());
    }

    // Add one full duration period to the current spec value.
    let current_duration = instance.spec.duration_seconds.unwrap_or(extension_duration);
    let new_duration = current_duration.saturating_add(extension_duration);

    if let Err(err) = k8s_instances::extend_instance_duration(
        &state.kube,
        &instance_ns,
        &instance_name,
        new_duration,
    )
    .await
    {
        warn!(error = %err, "failed to extend instance duration");
        redirect_with("bad", "failed to extend instance");
        return Ok(());
    }

    // Audit log: instance extended
    let m = metrics::get();
    m.instances_extended
        .add(1, &m.attrs(&[metrics::challenge_kv(&challenge_name)]));
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

    redirect_with("ok", "instance duration extended");
    Ok(())
}

// ---------------------------------------------------------------------------
// Instance state (JSON poll endpoint)
// ---------------------------------------------------------------------------

pub async fn instance_state(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<db::User>,
    Path(challenge_name): Path<String>,
) -> Response {
    let ctf_name = &state.config.ctf_name;

    let no_instance = InstanceStateResponse {
        has_instance: false,
        ready: false,
        can_extend: false,
        solved: false,
        deleting: false,
        time_left_seconds: None,
        expires_at_epoch: None,
        instance_name: None,
    };

    let Some(challenge) = challenges::get_challenge(&state, &challenge_name) else {
        return (axum::http::StatusCode::NOT_FOUND, Json(no_instance)).into_response();
    };

    if !has_ctf_access(&user, &challenge, &state.config) {
        return (axum::http::StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let solved = db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, ctf_name)
        .await
        .unwrap_or(false);

    let Some(instance) =
        k8s_instances::get_instance_for_challenge(&state, user.id, &challenge_name, ctf_name)
    else {
        return Json(InstanceStateResponse {
            solved,
            ..no_instance
        })
        .into_response();
    };

    if !is_instance_owned_by_user(&instance, &user) {
        return Json(InstanceStateResponse {
            solved,
            ..no_instance
        })
        .into_response();
    }

    let ready = instance_ready_from_resource(&instance);
    let time_left_seconds = instance_time_left_seconds(&instance);
    let expires_at_epoch = instance_expires_at_epoch(&instance);
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
        solved,
        deleting,
        time_left_seconds,
        expires_at_epoch,
        instance_name: Some(instance.name_any()),
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Stop (delete) instance (server function)
// ---------------------------------------------------------------------------

#[server(endpoint = "stop_instance")]
pub async fn stop_instance(challenge_name: String) -> Result<(), ServerFnError> {
    let state = expect_context::<Arc<AppState>>();
    let Some(user) = use_context::<db::User>() else {
        leptos_axum::redirect("/login");
        return Ok(());
    };

    let headers: HeaderMap = leptos_axum::extract()
        .await
        .map_err(|e| ServerFnError::new(format!("headers: {e}")))?;
    let socket_addr = leptos_axum::extract::<ConnectInfo<SocketAddr>>()
        .await
        .ok()
        .map(|ci| ci.0);

    let ctf_name = &state.config.ctf_name;
    let client_ip = extract_client_ip(state.config.behind_reverse_proxy, &headers, socket_addr);

    // Global challenges cannot be stopped by users
    let instancing = challenges::get_challenge(&state, &challenge_name)
        .map(|c| c.instancing)
        .unwrap_or(true);
    if !instancing {
        leptos_axum::redirect(&challenge_route(&challenge_name));
        return Ok(());
    }

    let redirect = || leptos_axum::redirect(&challenge_route(&challenge_name));

    let Some(instance) =
        k8s_instances::get_instance_for_challenge(&state, user.id, &challenge_name, ctf_name)
    else {
        redirect();
        return Ok(());
    };

    // Verify ownership before deleting
    if !is_instance_owned_by_user(&instance, &user) {
        redirect();
        return Ok(());
    };

    let instance_name = instance.name_any();
    let instance_ns = instance.metadata.namespace.clone().unwrap_or_default();

    // Delete the instance from cluster (initiates termination)
    if let Err(err) =
        k8s_instances::delete_instance(&state.kube, &instance_ns, &instance_name).await
    {
        warn!(error = %err, "failed to stop instance");
        redirect();
        return Ok(());
    }

    // Audit log: instance stopped
    let m = metrics::get();
    m.instances_stopped
        .add(1, &m.attrs(&[metrics::challenge_kv(&challenge_name)]));
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

    redirect();
    Ok(())
}

// ---------------------------------------------------------------------------
// SSE – push instance state changes to connected clients
// ---------------------------------------------------------------------------

/// Compute a compact state key for change detection.
fn instance_state_key(resp: &InstanceStateResponse) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        resp.solved,
        resp.has_instance,
        resp.ready,
        resp.deleting,
        resp.expires_at_epoch.unwrap_or(0),
        resp.instance_name.as_deref().unwrap_or(""),
    )
}

/// Compute [`InstanceStateResponse`] from the in-memory kube store.
fn compute_instance_state(
    state: &AppState,
    user_id: i64,
    challenge_name: &str,
    solved: bool,
) -> InstanceStateResponse {
    let no_instance = InstanceStateResponse {
        has_instance: false,
        ready: false,
        can_extend: false,
        solved,
        deleting: false,
        time_left_seconds: None,
        expires_at_epoch: None,
        instance_name: None,
    };

    let ctf_name = &state.config.ctf_name;
    let Some(instance) =
        k8s_instances::get_instance_for_challenge(state, user_id, challenge_name, ctf_name)
    else {
        return no_instance;
    };

    if !is_instance_owned_by_user_by_id(&instance, user_id) {
        return no_instance;
    }

    let ready = instance_ready_from_resource(&instance);
    let time_left_seconds = instance_time_left_seconds(&instance);
    let expires_at_epoch = instance_expires_at_epoch(&instance);
    let deleting = is_instance_deleting(&instance);
    let can_extend = ready
        && !deleting
        && time_left_seconds
            .map(|seconds| seconds <= 300 && seconds > 0)
            .unwrap_or(false);

    InstanceStateResponse {
        has_instance: true,
        ready,
        can_extend,
        solved,
        deleting,
        time_left_seconds,
        expires_at_epoch,
        instance_name: Some(instance.name_any()),
    }
}

/// Check instance ownership by user ID (used in SSE stream where we don't
/// have the full `User` struct).
fn is_instance_owned_by_user_by_id(instance: &Instance, user_id: i64) -> bool {
    match &instance.spec.user {
        Some(u) => u.id == user_id,
        None => true,
    }
}

/// SSE endpoint: pushes instance state events only when something changes.
///
/// The server reads from the in-memory kube reflector store every 3 s (cheap
/// HashMap lookup) and only sends an event when the state key differs from
/// the previous one.  This means **zero** traffic when the instance is
/// steady-state and only brief bursts during transitions.
pub async fn instance_events(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<db::User>,
    Path(challenge_name): Path<String>,
) -> Response {
    let Some(challenge) = challenges::get_challenge(&state, &challenge_name) else {
        return (axum::http::StatusCode::NOT_FOUND, "challenge not found").into_response();
    };

    if !has_ctf_access(&user, &challenge, &state.config) {
        return (axum::http::StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let user_id = user.id;
    let ctf_name = state.config.ctf_name.clone();
    let challenge_name_owned = challenge_name.clone();

    let stream = async_stream::stream! {
        // Send the initial state immediately so the client doesn't start blind.
        let solved = db::is_challenge_solved_for_user(&state.db, user_id, &challenge_name_owned, &ctf_name)
            .await
            .unwrap_or(false);
        let snap = compute_instance_state(&state, user_id, &challenge_name_owned, solved);
        let mut last_key = instance_state_key(&snap);
        if let Ok(json) = serde_json::to_string(&snap) {
            yield Ok::<_, Infallible>(Event::default().data(json));
        }

        let mut interval = tokio::time::interval(Duration::from_secs(3));
        loop {
            interval.tick().await;

            let solved = db::is_challenge_solved_for_user(&state.db, user_id, &challenge_name_owned, &ctf_name)
                .await
                .unwrap_or(false);
            let snap = compute_instance_state(&state, user_id, &challenge_name_owned, solved);
            let key = instance_state_key(&snap);

            if key != last_key {
                last_key = key;
                if let Ok(json) = serde_json::to_string(&snap) {
                    yield Ok::<_, Infallible>(Event::default().data(json));
                }
            }
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
        .into_response()
}
