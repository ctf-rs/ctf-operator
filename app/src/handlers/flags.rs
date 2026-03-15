use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::ConnectInfo,
    http::HeaderMap,
};
use ctf_cli::k8s::Flag;
use ctf_cli::models::Flag as CoreFlag;
use kube::{ResourceExt, runtime::reflector::ObjectRef};
use leptos::prelude::*;
use tracing::warn;

use crate::{
    db,
    helpers::{challenge_route, extract_client_ip},
    k8s::{
        challenges,
        helpers::is_instance_owned_by_user,
        instances::{self as k8s_instances, delete_instance},
    },
    state::AppState,
};

// ---------------------------------------------------------------------------
// Submit flag (server function)
// ---------------------------------------------------------------------------

#[server(endpoint = "submit_flag")]
pub async fn submit_flag(
    challenge_name: String,
    flag_name: String,
    submitted_flag: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<Arc<AppState>>();
    let Some(user) = use_context::<db::User>() else {
        leptos_axum::redirect("/login");
        return Ok(());
    };

    let headers: HeaderMap = leptos_axum::extract().await
        .map_err(|e| ServerFnError::new(format!("headers: {e}")))?;
    let connect_info: ConnectInfo<SocketAddr> = leptos_axum::extract().await
        .unwrap_or(ConnectInfo(SocketAddr::from(([0, 0, 0, 0], 0))));

    let ctf_name = &state.config.ctf_name;
    let client_ip = extract_client_ip(state.config.behind_reverse_proxy, &headers, Some(connect_info.0));

    let submitted_flag_trimmed = submitted_flag.trim();

    let redirect_with = |submit: &str, message: &str| {
        leptos_axum::redirect(&format!(
            "{}?submit={}&flag={}&message={}",
            challenge_route(&challenge_name),
            submit,
            urlencoding::encode(&flag_name),
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
        k8s_instances::get_instance_for_user(&state, user.id, &challenge_name, ctf_name)
    else {
        persist_flag_with_audit(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &flag_name,
            submitted_flag_trimmed,
            false,
            client_ip.as_deref(),
        )
        .await;
        redirect_with("bad", "spawn an instance first");
        return Ok(());
    };

    let instance_ns = instance.metadata.namespace.clone().unwrap_or_default();
    let instance_name = instance.name_any();

    if !is_instance_owned_by_user(&instance, &user) {
        persist_flag_with_audit(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &flag_name,
            submitted_flag_trimmed,
            false,
            client_ip.as_deref(),
        )
        .await;
        redirect_with("bad", "instance belongs to a different user");
        return Ok(());
    };

    let Some(uid) = instance.metadata.uid.clone() else {
        persist_flag_with_audit(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &flag_name,
            submitted_flag_trimmed,
            false,
            client_ip.as_deref(),
        )
        .await;
        redirect_with("bad", "instance is not ready yet");
        return Ok(());
    };

    let runtime_ns = format!("ctf-inst-{uid}");
    let flag_obj_name = format!("{}-{}", instance_name, flag_name);
    let flag_obj_ref = ObjectRef::<Flag>::new(&flag_obj_name).within(&runtime_ns);

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
            core_flag.verify(submitted_flag_trimmed)
        }
        None => false,
    };

    persist_flag_with_audit(
        &state,
        user.id,
        &challenge_name,
        ctf_name,
        &flag_name,
        submitted_flag_trimmed,
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

        let total_flags = challenges::get_challenge(&state, &challenge_name)
            .map(|ch| ch.flags.len())
            .unwrap_or(1);

        let all_flags_solved = solved_flag_count >= total_flags;

        if all_flags_solved {
            if let Err(err) =
                db::mark_challenge_solved(&state.db, user.id, &challenge_name, ctf_name).await
            {
                warn!(error = %err, "failed to persist solved challenge state");
            }

            if let Err(err) = delete_instance(&state.kube, &instance_ns, &instance_name).await {
                warn!(error = %err, "failed to delete solved instance");
            }

            redirect_with("good", "challenge solved");
            return Ok(());
        }

        // Flag accepted but more flags remain
        redirect_with("good", "flag accepted");
        return Ok(());
    }

    redirect_with("bad", "flag is not correct");
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Persist flag submission with cheat detection
async fn persist_flag_with_audit(
    state: &Arc<AppState>,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
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

    if let Err(err) = db::add_flag_submission(
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
        warn!(error = %err, "failed to persist flag submission");
    }
}
