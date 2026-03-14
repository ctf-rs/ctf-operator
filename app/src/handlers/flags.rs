use std::sync::Arc;

use axum::{
    extract::{Form, Path, State},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use ctf_cli::k8s::{Flag, Instance};
use ctf_cli::models::Flag as CoreFlag;
use kube::Api;
use serde::Deserialize;
use tracing::warn;

use crate::{
    db,
    helpers::{challenge_route, current_user},
    k8s::helpers::is_instance_owned_by_user,
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
    Path(challenge_name): Path<String>,
    Form(form): Form<SubmitFlagForm>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    let ctf_name = &state.config.ctf_name;

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

    let Some(instance) =
        db::get_instance_for_user(&state.db, user.id, &challenge_name, ctf_name)
            .await
            .ok()
            .flatten()
    else {
        persist_flag(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &challenge_namespace,
            &form.flag_name,
            submitted_flag,
            false,
        )
        .await;
        return redirect_with("bad", "spawn an instance first").into_response();
    };

    let api: Api<Instance> = Api::namespaced(state.kube.clone(), &instance.namespace);
    let Ok(instance_obj) = api.get(&instance.instance_name).await else {
        persist_flag(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &instance.namespace,
            &form.flag_name,
            submitted_flag,
            false,
        )
        .await;
        return redirect_with("bad", "instance not found").into_response();
    };

    if !is_instance_owned_by_user(&instance_obj, &user) {
        persist_flag(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &instance.namespace,
            &form.flag_name,
            submitted_flag,
            false,
        )
        .await;
        return redirect_with("bad", "instance belongs to a different user").into_response();
    }

    let Some(uid) = instance_obj.metadata.uid.clone() else {
        persist_flag(
            &state,
            user.id,
            &challenge_name,
            ctf_name,
            &instance.namespace,
            &form.flag_name,
            submitted_flag,
            false,
        )
        .await;
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
                scoring: flag_obj.spec.scoring,
            };
            core_flag.verify(submitted_flag)
        }
        Err(_) => false,
    };

    persist_flag(
        &state,
        user.id,
        &challenge_name,
        ctf_name,
        &instance.namespace,
        &form.flag_name,
        submitted_flag,
        accepted,
    )
    .await;

    if accepted {
        // Check if all flags are now solved
        let solved_flag_count = db::count_solved_flags_for_user(
            &state.db,
            user.id,
            &challenge_name,
            ctf_name,
        )
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

async fn persist_flag(
    state: &Arc<AppState>,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    challenge_namespace: &str,
    flag_name: &str,
    submitted_flag: &str,
    accepted: bool,
) {
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
}
