use std::{net::SocketAddr, sync::Arc};

use axum::{extract::ConnectInfo, http::HeaderMap};
use ctf_cli::k8s::Flag;
use ctf_cli::models::Flag as CoreFlag;
use kube::{ResourceExt, runtime::reflector::ObjectRef};
use leptos::prelude::*;
use tracing::warn;

use crate::{
    db,
    helpers::{challenge_route, extract_client_ip, get_ctf_schedule, is_admin, is_ctf_ended},
    k8s::{
        challenges,
        helpers::is_instance_owned_by_user,
        instances::{self as k8s_instances, delete_instance},
    },
    metrics,
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

    let headers: HeaderMap = leptos_axum::extract()
        .await
        .map_err(|e| ServerFnError::new(format!("headers: {e}")))?;
    let socket_addr = leptos_axum::extract::<ConnectInfo<SocketAddr>>()
        .await
        .ok()
        .map(|ci| ci.0);

    let ctf_name = &state.config.ctf_name;
    let client_ip = extract_client_ip(state.config.behind_reverse_proxy, &headers, socket_addr);
    let admin_submission = is_admin(&user, &state.config);

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

    // Block flag submission when the CTF has ended
    let schedule = get_ctf_schedule(&state);
    if is_ctf_ended(&schedule) {
        redirect_with("bad", "the CTF has ended");
        return Ok(());
    }

    if db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, ctf_name)
        .await
        .unwrap_or(false)
    {
        redirect_with("bad", "challenge already solved");
        return Ok(());
    }

    // ---- Container-less challenges: verify flag directly from spec --------
    let challenge_crd = state
        .challenges
        .state()
        .into_iter()
        .find(|c| c.name_any() == challenge_name);

    let has_containers = challenge_crd
        .as_ref()
        .map_or(true, |c| !c.spec.containers.is_empty());

    if !has_containers {
        let (kind, msg) = submit_static_flag(
            &state,
            &challenge_crd,
            user.id,
            &challenge_name,
            ctf_name,
            &flag_name,
            submitted_flag_trimmed,
            admin_submission,
            client_ip.as_deref(),
        )
        .await;
        redirect_with(&kind, &msg);
        return Ok(());
    }

    let Some(instance) =
        k8s_instances::get_instance_for_challenge(&state, user.id, &challenge_name, ctf_name)
    else {
        persist_flag_submission(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &flag_name,
            submitted_flag_trimmed,
            false,
            admin_submission,
            client_ip.as_deref(),
        )
        .await;
        redirect_with("bad", "spawn an instance first");
        return Ok(());
    };

    let instance_ns = instance.metadata.namespace.clone().unwrap_or_default();
    let instance_name = instance.name_any();

    if !is_instance_owned_by_user(&instance, &user) {
        persist_flag_submission(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &flag_name,
            submitted_flag_trimmed,
            false,
            admin_submission,
            client_ip.as_deref(),
        )
        .await;
        redirect_with("bad", "instance belongs to a different user");
        return Ok(());
    };

    if instance.metadata.uid.is_none() {
        persist_flag_submission(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &flag_name,
            submitted_flag_trimmed,
            false,
            admin_submission,
            client_ip.as_deref(),
        )
        .await;
        redirect_with("bad", "instance is not ready yet");
        return Ok(());
    }

    let runtime_ns = instance_name.to_string();
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

    // ---- Cross-instance / cheat detection --------------------------------
    // Skip cheat detection for non-instanced (global) challenges — all
    // users share the same instance and flags, so cross-instance checks
    // are meaningless.
    let instancing = challenges::get_challenge(&state, &challenge_name)
        .map(|c| c.instancing)
        .unwrap_or(true);

    let (accepted, cheated, cheated_instance_flag_id) = if accepted || !instancing {
        // Matched current instance, or non-instanced challenge (no cheat check)
        (accepted, false, None)
    } else {
        // Check if the submitted value matches any previously generated flag
        match db::check_flag_origin(&state.db, submitted_flag_trimmed, ctf_name).await {
            Ok(Some(origin)) => {
                let same_user = origin.user_id == Some(user.id);
                let same_challenge = origin.challenge_name == challenge_name;

                match (same_user, same_challenge) {
                    // Own flag from an older instance, same challenge → accept
                    (true, true) => (true, false, Some(origin.id)),
                    // Own flag from a different challenge → just wrong input
                    (true, false) => (false, false, None),
                    // Another user's flag, same challenge → accept + cheated
                    (false, true) => {
                        warn!(
                            user_id = user.id,
                            origin_user_id = ?origin.user_id,
                            instance_flag_id = origin.id,
                            "CHEAT DETECTED: flag from another user's instance"
                        );
                        (true, true, Some(origin.id))
                    }
                    // Another user's flag, different challenge → reject + cheated
                    (false, false) => {
                        warn!(
                            user_id = user.id,
                            origin_user_id = ?origin.user_id,
                            instance_flag_id = origin.id,
                            "CHEAT DETECTED: flag from another user's different challenge"
                        );
                        (false, true, Some(origin.id))
                    }
                }
            }
            Ok(None) => (false, false, None),
            Err(err) => {
                warn!(error = %err, "failed to check flag origin");
                (false, false, None)
            }
        }
    };

    // ---- Persist submission + metrics ----------------------------------
    if let Err(err) = db::add_flag_submission(
        &state.db,
        user.id,
        &challenge_name,
        ctf_name,
        &flag_name,
        submitted_flag_trimmed,
        accepted,
        cheated,
        cheated_instance_flag_id,
        admin_submission,
        client_ip.as_deref(),
    )
    .await
    {
        warn!(error = %err, "failed to persist flag submission");
    }

    let m = metrics::get();
    if cheated {
        m.cheats_detected
            .add(1, &m.attrs(&[metrics::challenge_kv(&challenge_name)]));
        m.flag_submissions.add(
            1,
            &m.attrs(&[
                metrics::challenge_kv(&challenge_name),
                opentelemetry::KeyValue::new("result", "cheated"),
            ]),
        );
    }

    if accepted {
        m.flag_submissions.add(
            1,
            &m.attrs(&[
                metrics::challenge_kv(&challenge_name),
                opentelemetry::KeyValue::new("result", "correct"),
            ]),
        );

        // Check if all flags are now solved
        let solved_flag_count =
            db::count_solved_flags_for_user(&state.db, user.id, &challenge_name, ctf_name)
                .await
                .unwrap_or(0) as usize;

        let total_flags = challenges::get_challenge(&state, &challenge_name)
            .map(|ch| ch.flags.len())
            .unwrap_or(1);

        if solved_flag_count >= total_flags {
            m.challenges_solved
                .add(1, &m.attrs(&[metrics::challenge_kv(&challenge_name)]));
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

        redirect_with(
            "good",
            if cheated {
                "flag accepted (flagged for review)"
            } else {
                "flag accepted"
            },
        );
        return Ok(());
    }

    if !cheated {
        m.flag_submissions.add(
            1,
            &m.attrs(&[
                metrics::challenge_kv(&challenge_name),
                opentelemetry::KeyValue::new("result", "incorrect"),
            ]),
        );
    }

    redirect_with("bad", "flag is not correct");
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Handle flag submission for challenges that have no containers.
///
/// Flags are verified directly from the Challenge CRD spec (static flags)
/// instead of requiring a running instance.  Returns `(kind, message)` for
/// the redirect feedback.
#[expect(clippy::too_many_arguments)]
async fn submit_static_flag(
    state: &Arc<AppState>,
    challenge_crd: &Option<std::sync::Arc<ctf_cli::k8s::Challenge>>,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    flag_name: &str,
    submitted_flag: &str,
    admin_submission: bool,
    client_ip: Option<&str>,
) -> (String, String) {
    let Some(challenge_crd) = challenge_crd else {
        persist_flag_submission(
            state,
            user_id,
            challenge_name,
            ctf_name,
            flag_name,
            submitted_flag,
            false,
            admin_submission,
            client_ip,
        )
        .await;
        return ("bad".into(), "challenge not found".into());
    };

    // Find the matching flag definition in the spec
    let challenge_flag = challenge_crd
        .spec
        .flags
        .iter()
        .find(|f| f.name == flag_name);

    let accepted = if let Some(cf) = challenge_flag {
        // Look up CTF flag defaults for prefix/format
        let ctf_crd = state
            .ctfs
            .state()
            .into_iter()
            .find(|c| c.name_any() == *ctf_name);

        if let Some(ctf_crd) = ctf_crd {
            let core_flag = CoreFlag {
                prefix: cf
                    .prefix
                    .clone()
                    .unwrap_or_else(|| ctf_crd.spec.flag.prefix.clone()),
                format: cf.format.unwrap_or(ctf_crd.spec.flag.format),
                body: cf.body.clone(),
                random: cf
                    .random
                    .clone()
                    .or_else(|| ctf_crd.spec.flag.random.clone()),
                options: ctf_cli::models::FlagMatchOptions {
                    ignore_case: cf.options.ignore_case || ctf_crd.spec.flag.options.ignore_case,
                    ignore_random: cf.options.ignore_random
                        || ctf_crd.spec.flag.options.ignore_random,
                },
                scoring: cf.scoring.clone(),
            };
            core_flag.verify(submitted_flag)
        } else {
            false
        }
    } else {
        false
    };

    // Persist submission
    if let Err(err) = db::add_flag_submission(
        &state.db,
        user_id,
        challenge_name,
        ctf_name,
        flag_name,
        submitted_flag,
        accepted,
        false,
        None,
        admin_submission,
        client_ip,
    )
    .await
    {
        warn!(error = %err, "failed to persist flag submission");
    }

    let m = metrics::get();

    if accepted {
        m.flag_submissions.add(
            1,
            &m.attrs(&[
                metrics::challenge_kv(challenge_name),
                opentelemetry::KeyValue::new("result", "correct"),
            ]),
        );

        // Check if all flags are now solved
        let solved_flag_count =
            db::count_solved_flags_for_user(&state.db, user_id, challenge_name, ctf_name)
                .await
                .unwrap_or(0) as usize;

        let total_flags = challenges::get_challenge(state, challenge_name)
            .map(|ch| ch.flags.len())
            .unwrap_or(1);

        if solved_flag_count >= total_flags {
            m.challenges_solved
                .add(1, &m.attrs(&[metrics::challenge_kv(challenge_name)]));
            if let Err(err) =
                db::mark_challenge_solved(&state.db, user_id, challenge_name, ctf_name).await
            {
                warn!(error = %err, "failed to persist solved challenge state");
            }
            return ("good".into(), "challenge solved".into());
        }

        return ("good".into(), "flag accepted".into());
    }

    m.flag_submissions.add(
        1,
        &m.attrs(&[
            metrics::challenge_kv(challenge_name),
            opentelemetry::KeyValue::new("result", "incorrect"),
        ]),
    );

    ("bad".into(), "flag is not correct".into())
}

/// Persist a flag submission (no cheat logic — that lives in `submit_flag`).
async fn persist_flag_submission(
    state: &Arc<AppState>,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    flag_name: &str,
    submitted_flag: &str,
    accepted: bool,
    admin_submission: bool,
    ip_address: Option<&str>,
) {
    if let Err(err) = db::add_flag_submission(
        &state.db,
        user_id,
        challenge_name,
        ctf_name,
        flag_name,
        submitted_flag,
        accepted,
        false,
        None,
        admin_submission,
        ip_address,
    )
    .await
    {
        warn!(error = %err, "failed to persist flag submission");
    }
}
