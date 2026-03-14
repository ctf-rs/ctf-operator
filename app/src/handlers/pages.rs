use std::{collections::HashSet, sync::Arc};

use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use ctf_cli::{ChallengeCategory, k8s::{Instance, PortClaim}};
use kube::Api;
use serde::Deserialize;
use tracing::warn;

use crate::{
    db,
    helpers::{current_user, has_ctf_access},
    k8s::helpers::{
        instance_phase_description, instance_ready_from_resource, instance_time_left_seconds,
        is_instance_owned_by_user,
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

    // Get partial solve counts to detect challenges with some flags submitted
    let partial_solve_counts = db::get_partial_solve_counts_for_user(&state.db, user.id)
        .await
        .unwrap_or_default();

    // Get all instances for this user and fetch their time left from k8s
    let instances = db::list_instances_for_user(&state.db, user.id)
        .await
        .unwrap_or_default();

    // Build a map of (ctf_name, challenge_name) -> time_left_seconds
    let mut instance_times: std::collections::HashMap<(String, String), Option<u64>> =
        std::collections::HashMap::new();

    for inst in instances {
        let api: Api<Instance> = Api::namespaced(state.kube.clone(), &inst.namespace);
        match api.get(&inst.instance_name).await {
            Ok(instance_obj) => {
                let time_left = instance_time_left_seconds(&instance_obj);
                instance_times.insert((inst.ctf_name, inst.challenge_name), time_left);
            }
            Err(_) => {
                // Instance no longer exists in k8s, mark as no instance
            }
        }
    }

    // Build category → cards map. Challenges with multiple categories appear in all of them.
    let mut category_map: std::collections::BTreeMap<ChallengeCategory, Vec<views::ChallengeCardData>> =
        std::collections::BTreeMap::new();

    for challenge in challenges {
        let key = format!("{}::{}", challenge.ctf_name, challenge.name);
        let solved = solved_keys.contains(&key);
        let total_flags = challenge.flags.len();

        // Get solved flags count
        let solved_flags = if solved {
            total_flags
        } else {
            partial_solve_counts
                .get(&(challenge.ctf_name.clone(), challenge.name.clone()))
                .copied()
                .unwrap_or(0) as usize
        };

        // Partial solve: some flags submitted but not fully solved
        let partial_solve = !solved && solved_flags > 0;

        // Check for instance
        let instance_key = (challenge.ctf_name.clone(), challenge.name.clone());
        let (has_instance, time_left_seconds) =
            if let Some(time_left) = instance_times.get(&instance_key) {
                (true, *time_left)
            } else {
                (false, None)
            };

        let cats: Vec<ChallengeCategory> = if challenge.categories.is_empty() {
            vec![ChallengeCategory::Misc]
        } else {
            challenge.categories.clone()
        };
        for cat in cats {
            category_map
                .entry(cat)
                .or_default()
                .push(views::ChallengeCardData {
                    challenge: challenge.clone(),
                    solved,
                    partial_solve,
                    solved_flags,
                    total_flags,
                    time_left_seconds,
                    has_instance,
                });
        }
    }

    // Sort cards within each category by difficulty (veryeasy → insane)
    for cards in category_map.values_mut() {
        cards.sort_by_key(|card| card.challenge.difficulty.map(|d| d.sort_key()).unwrap_or(u8::MAX));
    }

    // Build sorted category sections
    let mut sections: Vec<views::CategorySection> = category_map
        .into_iter()
        .map(|(cat, cards)| views::CategorySection {
            label: cat.label().to_string(),
            category: cat,
            cards,
        })
        .collect();
    sections.sort_by_key(|s| s.category.sort_key());

    let user_label = user
        .name
        .clone()
        .or(user.email.clone())
        .unwrap_or_else(|| user.sub.clone());

    let ctf_title = ctf_name.clone();
    let html = views::render_to_html(move || {
        leptos::prelude::view! {
            <views::IndexPage ctf_title=ctf_title user_label=user_label sections=sections/>
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
    headers: axum::http::HeaderMap,
    Path(challenge_name): Path<String>,
    Query(query): Query<ChallengePageQuery>,
) -> Response {
    let user = match current_user(&state, &jar).await {
        Ok(Some(user)) => user,
        _ => return Redirect::to("/").into_response(),
    };

    // Extract hostname (without port) from Host header for connection details
    let request_hostname = headers
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| h.split(':').next().unwrap_or(h))
        .unwrap_or("localhost")
        .to_string();

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

    // Mark which flags are already solved by this user
    let solved_flags = db::get_solved_flags_for_user(&state.db, user.id, &challenge_name, ctf_name)
        .await
        .unwrap_or_default();
    let mut challenge = challenge;
    for flag in &mut challenge.flags {
        flag.solved = solved_flags.contains(&flag.name);
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
    let mut instance_phase_desc: Option<String> = None;

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
                instance_phase_desc = Some(instance_phase_description(&instance_obj));

                // Fetch allocated ports from PortClaims when instance is ready
                if ready {
                    if let Some(uid) = &instance_obj.metadata.uid {
                        let target_ns = format!("{}{}", state.config.instance_namespace_prefix, uid);
                        let port_api: Api<PortClaim> = Api::namespaced(state.kube.clone(), &target_ns);
                        if let Ok(port_claims) = port_api.list(&Default::default()).await {
                            for claim in port_claims.items {
                                if let Some(port_status) = claim.status {
                                    if let Some(allocated) = port_status.port {
                                        let claim_name = claim.metadata.name.unwrap_or_default();
                                        // PortClaim name format: {instance_name}-{port_name}
                                        if let Some(port_name) = claim_name.strip_prefix(&format!("{}-", existing.instance_name)) {
                                            for port in &mut challenge.exposed_ports {
                                                if port.name == port_name {
                                                    port.allocated_port = Some(allocated);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
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

    let instance_name = instance.as_ref().map(|i| i.instance_name.clone());
    // Use config override if set, otherwise use the hostname from the request
    let instance_base_domain = state.config.instance_base_domain.clone()
        .unwrap_or(request_hostname);

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
        instance_name,
        instance_base_domain,
        instance_phase_desc,
    };

    let html = views::render_to_html(move || {
        leptos::prelude::view! { <views::ChallengeDetailPage data=page_data/> }
    });

    Html(html).into_response()
}
