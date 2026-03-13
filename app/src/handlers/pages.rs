use std::{collections::HashSet, sync::Arc};

use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use ctf_cli::k8s::Instance;
use kube::Api;
use serde::Deserialize;
use tracing::warn;

use crate::{
    db,
    helpers::{current_user, has_ctf_access},
    k8s::helpers::{
        instance_ready_from_resource, instance_time_left_seconds, is_instance_owned_by_user,
    },
    state::AppState,
    views,
};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ChallengePageQuery {
    submit: Option<String>,
    flag: Option<String>,
    message: Option<String>,
    extend: Option<String>,
}

// ---------------------------------------------------------------------------
// Index (challenge list)
// ---------------------------------------------------------------------------

pub async fn index(State(state): State<Arc<AppState>>, jar: CookieJar) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(user) => user,
        Err(err) => {
            warn!(error = %err, "failed to read current user");
            None
        }
    };

    let ctf_name = state.config.ctf_name.clone();

    let Some(user) = user else {
        let html = views::render_to_html(move || {
            leptos::prelude::view! { <views::LoginPage ctf_title=ctf_name/> }
        });
        return Html(html).into_response();
    };

    let challenges = match db::list_challenges_for_ctf(&state.db, &ctf_name).await {
        Ok(challenges) => challenges,
        Err(err) => {
            warn!(error = %err, "failed to list challenges");
            Vec::new()
        }
    };

    let challenges = challenges
        .into_iter()
        .filter(|challenge| has_ctf_access(&user, challenge))
        .collect::<Vec<_>>();

    let solved_keys = db::list_solved_challenges_for_user(&state.db, user.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|entry| format!("{}::{}", entry.ctf_name, entry.challenge_name))
        .collect::<HashSet<_>>();

    let cards: Vec<views::ChallengeCardData> = challenges
        .into_iter()
        .map(|challenge| {
            let solved =
                solved_keys.contains(&format!("{}::{}", challenge.ctf_name, challenge.name));
            views::ChallengeCardData { challenge, solved }
        })
        .collect();

    let user_label = user
        .name
        .clone()
        .or(user.email.clone())
        .unwrap_or_else(|| user.sub.clone());

    let ctf_title = ctf_name.clone();
    let html = views::render_to_html(move || {
        leptos::prelude::view! {
            <views::IndexPage ctf_title=ctf_title user_label=user_label cards=cards/>
        }
    });

    Html(html).into_response()
}

// ---------------------------------------------------------------------------
// Challenge detail page
// ---------------------------------------------------------------------------

pub async fn challenge_page(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(challenge_name): Path<String>,
    Query(query): Query<ChallengePageQuery>,
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

    let solved = db::is_challenge_solved_for_user(&state.db, user.id, &challenge_name, ctf_name)
        .await
        .unwrap_or(false);

    let mut instance = db::get_instance_for_user(&state.db, user.id, &challenge_name, ctf_name)
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
            }
            Err(_) => {}
        }
    }

    let user_label = user
        .name
        .clone()
        .or(user.email.clone())
        .unwrap_or_else(|| user.sub.clone());

    let submit_feedback = views::parse_submission_feedback(
        query.submit.as_deref(),
        query.flag.as_deref(),
        query.message.as_deref(),
    );
    let extend_feedback =
        views::parse_extend_feedback(query.extend.as_deref(), query.message.as_deref());

    let page_data = views::ChallengePageData {
        challenge,
        ctf_title: ctf_name.clone(),
        user_label,
        has_instance: instance.is_some(),
        ready,
        time_left_seconds,
        can_extend,
        solved,
        submit_feedback,
        extend_feedback,
    };

    let html = views::render_to_html(move || {
        leptos::prelude::view! { <views::ChallengeDetailPage data=page_data/> }
    });

    Html(html).into_response()
}
