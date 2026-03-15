use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, Form, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use ctf_cli::k8s::{Flag, Instance};
use ctf_cli::models::Flag as CoreFlag;
use kube::runtime::reflector::ObjectRef;
use serde::Deserialize;
use tracing::warn;

use crate::{
    db,
    helpers::{challenge_route, current_user, extract_client_ip},
    k8s::{helpers::is_instance_owned_by_user, instances::delete_instance},
    state::AppState,
};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SubmitFlagForm {
    flag_name: String,
    submitted_flag: String,
}

// ---------------------------------------------------------------------------
// Submit flag
// ---------------------------------------------------------------------------

pub async fn submit_flag(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(challenge_name): Path<String>,
    Form(form): Form<SubmitFlagForm>,
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
            "{}?submit=bad&flag={}&message={}",
            challenge_route(&challenge_name),
            urlencoding::encode(&form.flag_name),
            urlencoding::encode("challenge already solved")
        ))
        .into_response();
    }

    let submitted_flag = form.submitted_flag.trim();
    let challenge_namespace = db::find_challenge_namespace(&state.db, &challenge_name, ctf_name)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".to_string());

    let redirect_with = |submit: &str, message: &str| {
        Redirect::to(&format!(
            "{}?submit={}&flag={}&message={}",
            challenge_route(&challenge_name),
            submit,
            urlencoding::encode(&form.flag_name),
            urlencoding::encode(message)
        ))
    };

    let Some(instance) = db::get_instance_for_user(&state.db, user.id, &challenge_name, ctf_name)
        .await
        .ok()
        .flatten()
    else {
        persist_flag_with_audit(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &challenge_namespace,
            &form.flag_name,
            submitted_flag,
            false,
            client_ip.as_deref(),
        )
        .await;
        return redirect_with("bad", "spawn an instance first").into_response();
    };

    let obj_ref = ObjectRef::<Instance>::new(&instance.instance_name).within(&instance.namespace);
    let Some(instance_obj) = state.instances.get(&obj_ref) else {
        persist_flag_with_audit(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &instance.namespace,
            &form.flag_name,
            submitted_flag,
            false,
            client_ip.as_deref(),
        )
        .await;
        return redirect_with("bad", "instance not found").into_response();
    };

    if !is_instance_owned_by_user(&instance_obj, &user) {
        persist_flag_with_audit(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &instance.namespace,
            &form.flag_name,
            submitted_flag,
            false,
            client_ip.as_deref(),
        )
        .await;
        return redirect_with("bad", "instance belongs to a different user").into_response();
    };

    let Some(uid) = instance_obj.metadata.uid.clone() else {
        persist_flag_with_audit(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &instance.namespace,
            &form.flag_name,
            submitted_flag,
            false,
            client_ip.as_deref(),
        )
        .await;
        return redirect_with("bad", "instance is not ready yet").into_response();
    };

    let runtime_ns = format!("ctf-inst-{uid}");
    let flag_name = format!("{}-{}", instance.instance_name, form.flag_name);
    let flag_obj_ref = ObjectRef::<Flag>::new(&flag_name).within(&runtime_ns);

    let accepted = match state.flags.get(&flag_obj_ref) {
        Some(flag_obj) => {
            let core_flag = CoreFlag {
                prefix: flag_obj.spec.prefix.clone(),
                format: flag_obj.spec.format.clone(),
                body: flag_obj.spec.body.clone(),
                random: flag_obj.spec.random.clone(),
                options: flag_obj.spec.options.clone(),
                scoring: flag_obj.spec.scoring.clone(),
            };
            core_flag.verify(submitted_flag)
        }
        None => false,
    };

    persist_flag_with_audit(
        &state,
        user.id,
        &challenge_name,
        ctf_name,
        &instance.namespace,
        &form.flag_name,
        submitted_flag,
        accepted,
        client_ip.as_deref(),
    )
    .await;

    if accepted {
        // Check if all flags are now solved
        let solved_flag_count =
            db::count_solved_flags_for_user(&state.db, user.id, &challenge_name, ctf_name)
                .await
                .unwrap_or(0) as usize;

        let total_flags = db::get_challenge(&state.db, ctf_name, &challenge_name)
            .await
            .ok()
            .flatten()
            .map(|ch| ch.flags.len())
            .unwrap_or(1);

        let all_flags_solved = solved_flag_count >= total_flags;

        if all_flags_solved {
            if let Err(err) = db::mark_challenge_solved(
                &state.db,
                user.id,
                &instance.namespace,
                &challenge_name,
                ctf_name,
            )
            .await
            {
                warn!(error = %err, "failed to persist solved challenge state");
            }

            if let Err(err) =
                delete_instance(&state.kube, &instance.namespace, &instance.instance_name).await
            {
                warn!(error = %err, "failed to delete solved instance");
            }

            if let Err(err) =
                db::clear_instance_for_user(&state.db, user.id, &challenge_name, ctf_name).await
            {
                warn!(error = %err, "failed to clear solved instance relation");
            }

            return redirect_with("good", "challenge solved").into_response();
        }

        // Flag accepted but more flags remain
        return redirect_with("good", "flag accepted").into_response();
    }

    redirect_with("bad", "flag is not correct").into_response()
}

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Persist flag submission and log to audit trail with cheat detection
async fn persist_flag_with_audit(
    state: &Arc<AppState>,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    challenge_namespace: &str,
    flag_name: &str,
    submitted_flag: &str,
    accepted: bool,
    ip_address: Option<&str>,
) {
    // Check for cheating: if flag is not accepted, check if it was generated for another user
    let (cheated, cheated_from_user_id, cheated_flag_gen_id) = if !accepted {
        match db::check_flag_cheated(&state.db, submitted_flag, ctf_name, user_id).await {
            Ok(Some((flag_gen_id, from_user_id))) => {
                warn!(
                    user_id = user_id,
                    cheated_from_user_id = from_user_id,
                    flag_gen_id = flag_gen_id,
                    submitted_flag = submitted_flag,
                    "CHEAT DETECTED: user submitted flag belonging to another user"
                );
                (true, Some(from_user_id), Some(flag_gen_id))
            }
            Ok(None) => (false, None, None),
            Err(err) => {
                warn!(error = %err, "failed to check for cheated flag");
                (false, None, None)
            }
        }
    } else {
        (false, None, None)
    };

    // Log to the existing submissions table
    if let Err(err) = db::add_flag_submission(
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
    {
        warn!(error = %err, "failed to persist flag submission");
    }

    // Log to audit trail with full details
    if let Err(err) = db::log_submission(
        &state.db,
        user_id,
        challenge_name,
        ctf_name,
        flag_name,
        submitted_flag,
        accepted,
        cheated,
        cheated_from_user_id,
        cheated_flag_gen_id,
        ip_address,
    )
    .await
    {
        warn!(error = %err, "failed to log submission audit");
    }
}
