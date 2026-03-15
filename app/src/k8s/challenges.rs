//! Challenge operations using kube Store (no database queries).

use ctf_cli::k8s::ChallengeFileRef;
use kube::ResourceExt;

use crate::{
    db::models::{ChallengeView, FlagView, PortView},
    state::AppState,
};

/// Lists all challenges for a CTF, building ChallengeView from Store.
pub fn list_challenges_for_ctf(state: &AppState) -> Vec<ChallengeView> {
    let ctf_name = &state.config.ctf_name;

    // Find the CTF resource to get the list of enabled challenges
    let ctf = state
        .ctfs
        .state()
        .into_iter()
        .find(|c| c.name_any() == *ctf_name);

    let Some(ctf) = ctf else {
        return Vec::new();
    };

    let namespace = ctf
        .metadata
        .namespace
        .as_deref()
        .unwrap_or(&state.config.ctf_namespace);

    let mut challenges = Vec::new();

    for challenge_name in &ctf.spec.challenges {
        let Some(challenge) = state
            .challenges
            .state()
            .into_iter()
            .find(|c| c.name_any() == *challenge_name)
        else {
            continue;
        };

        if let Some(view) = build_challenge_view(&challenge, &ctf, namespace, state) {
            challenges.push(view);
        }
    }

    challenges.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    challenges
}

/// Gets a single challenge by name, building ChallengeView from Store.
pub fn get_challenge(state: &AppState, challenge_name: &str) -> Option<ChallengeView> {
    let ctf_name = &state.config.ctf_name;

    // Find the CTF resource
    let ctf = state
        .ctfs
        .state()
        .into_iter()
        .find(|c| c.name_any() == *ctf_name)?;

    // Check if this challenge is in the CTF's challenge list
    if !ctf.spec.challenges.iter().any(|c| c == challenge_name) {
        return None;
    }

    let namespace = ctf
        .metadata
        .namespace
        .as_deref()
        .unwrap_or(&state.config.ctf_namespace);

    let challenge = state
        .challenges
        .state()
        .into_iter()
        .find(|c| c.name_any() == challenge_name)?;

    build_challenge_view(&challenge, &ctf, namespace, state)
}

/// Finds the namespace a challenge lives in.
pub fn find_challenge_namespace(state: &AppState, challenge_name: &str) -> Option<String> {
    let ctf_name = &state.config.ctf_name;
    let ctf = state
        .ctfs
        .state()
        .into_iter()
        .find(|c| c.name_any() == *ctf_name)?;

    // Check if this challenge is in the CTF's challenge list
    if !ctf.spec.challenges.iter().any(|c| c == challenge_name) {
        return None;
    }

    ctf.metadata.namespace.clone()
}

fn build_challenge_view(
    challenge: &ctf_cli::k8s::Challenge,
    ctf: &ctf_cli::k8s::Ctf,
    namespace: &str,
    state: &AppState,
) -> Option<ChallengeView> {
    let files = challenge
        .spec
        .files
        .iter()
        .map(|file| match file {
            ChallengeFileRef::S3 {
                bucket,
                key,
                endpoint,
                path_style,
            } => {
                let effective_endpoint = endpoint
                    .as_deref()
                    .or(state.config.challenge_files_s3_endpoint.as_deref());
                let effective_path_style = if endpoint.is_some() {
                    *path_style
                } else {
                    state.config.challenge_files_s3_path_style
                };
                render_s3_url(bucket, key, effective_endpoint, effective_path_style)
            }
        })
        .collect::<Vec<_>>();

    // Per-flag difficulty: prefer explicit flag difficulty, otherwise fall back
    // to challenge-level difficulty (for backward compatibility with example
    // YAMLs that set difficulty per-challenge).
    let flags = challenge
        .spec
        .flags
        .iter()
        .map(|flag| FlagView {
            name: flag.name.clone(),
            label: flag.label.clone(),
            solved: false, // Will be populated by caller if needed
            base_points: flag.scoring.base_points,
            min_points: flag.scoring.min_points,
            decay: flag.scoring.decay,
            difficulty: flag.difficulty.or(challenge.spec.difficulty),
        })
        .collect::<Vec<_>>();

    // Collect exposed ports from all containers
    let exposed_ports: Vec<PortView> = challenge
        .spec
        .containers
        .iter()
        .flat_map(|container| {
            container
                .ports
                .iter()
                .filter(|p| p.expose)
                .map(|p| PortView {
                    name: p.name.clone(),
                    port: p.container_port,
                    port_type: format!("{:?}", p.port_type).to_lowercase(),
                    allocated_port: None, // Populated from PortClaim when viewing instance
                })
        })
        .collect();

    let categories: Vec<_> = challenge.spec.categories.iter().copied().collect();
    // Compute challenge-level difficulty as the easiest flag difficulty, if any
    let mut flag_diffs: Vec<_> = challenge
        .spec
        .flags
        .iter()
        .filter_map(|f| f.difficulty)
        .collect();
    flag_diffs.sort_by_key(|d| d.sort_key());
    let difficulty = flag_diffs.first().copied();

    Some(ChallengeView {
        namespace: namespace.to_string(),
        name: challenge.name_any(),
        ctf_name: ctf.name_any(),
        ctf_public: ctf.spec.public,
        full_name: challenge.spec.full_name.clone(),
        description: challenge.spec.description.clone(),
        categories,
        difficulty,
        files,
        flags,
        exposed_ports,
        instancing: challenge.spec.instancing,
        flag_hint: String::new(), // TODO: add to CRD
    })
}

fn render_s3_url(bucket: &str, key: &str, endpoint: Option<&str>, path_style: bool) -> String {
    let key = key.trim_start_matches('/');
    let Some(endpoint) = endpoint.map(str::trim).filter(|value| !value.is_empty()) else {
        return format!("https://{bucket}.s3.amazonaws.com/{key}");
    };

    let endpoint = endpoint.trim_end_matches('/');
    if path_style {
        return format!("{endpoint}/{bucket}/{key}");
    }

    if let Some(rest) = endpoint.strip_prefix("https://") {
        return format!("https://{bucket}.{rest}/{key}");
    }

    if let Some(rest) = endpoint.strip_prefix("http://") {
        return format!("http://{bucket}.{rest}/{key}");
    }

    // Fallback: assume https and virtual-hosted style
    format!("https://{bucket}.{endpoint}/{key}")
}
