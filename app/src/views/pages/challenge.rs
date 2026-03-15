use leptos::prelude::*;

use super::super::{
    ChallengePageData, components::InstanceControls, display_file_name, file_route,
    format_countdown, layout::PageShell, scripts::instance_live_script,
};

// ---------------------------------------------------------------------------
// Challenge detail page
// ---------------------------------------------------------------------------

#[component]
pub fn ChallengeDetailPage(data: ChallengePageData) -> impl IntoView {
    let ch = &data.challenge;
    let detail_class = if data.solved {
        "challenge-detail solved"
    } else {
        "challenge-detail"
    };

    let _spawn_action = "/api/spawn_instance";
    let _extend_action = "/api/extend_instance";
    let _stop_action = "/api/stop_instance";
    let submit_action = "/api/submit_flag";

    let full_name = ch.full_name.clone();
    let description = ch
        .description
        .clone()
        .unwrap_or_else(|| "No description".to_string());
    let challenge_name = ch.name.clone();
    let flags = ch.flags.clone();
    let flag_points = data.flag_points.clone();
    // Determine difficulties from flags (difficulty is now per-flag)
    let _difficulty = ch.difficulty;
    let exposed_ports = ch.exposed_ports.clone();
    let flag_hint = ch.flag_hint.clone();

    let files: Vec<(String, String)> = ch
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| (file_route(&ch.name, i), display_file_name(f)))
        .collect();

    let instancing = data.instancing;

    let _status_text = if data.solved {
        "Solved".to_string()
    } else if !instancing {
        // Global challenge
        if data.has_instance && data.ready {
            "Always On".to_string()
        } else if data.has_instance {
            if let Some(ref phase_desc) = data.instance_phase_desc {
                phase_desc.clone()
            } else {
                "Starting...".to_string()
            }
        } else {
            "Provisioning...".to_string()
        }
    } else if !data.has_instance {
        "No instance".to_string()
    } else if !data.ready {
        if data.time_left_seconds == Some(0) {
            "Expired".to_string()
        } else if let Some(ref phase_desc) = data.instance_phase_desc {
            phase_desc.clone()
        } else {
            "Starting...".to_string()
        }
    } else {
        "Ready".to_string()
    };

    // Only show timer when instanced, instance is ready and has time left
    let _show_timer = instancing && data.ready && data.time_left_seconds.is_some();
    let _timer_text = data
        .time_left_seconds
        .map(|s| format_countdown(s))
        .unwrap_or_default();

    let extend_display = if data.can_extend {
        "inline-block"
    } else {
        "none"
    };

    let instance_script = instance_live_script(&challenge_name);

    let _extend_fb = data.extend_feedback.clone();
    let submit_fb_flag_name = data.submit_feedback.as_ref().map(|f| f.flag_name.clone());
    let submit_fb_kind = data.submit_feedback.as_ref().map(|f| f.kind.clone());
    let submit_fb_message = data.submit_feedback.as_ref().map(|f| f.message.clone());

    let mut diff_list: Vec<_> = flags.iter().filter_map(|f| f.difficulty).collect();
    diff_list.sort_by_key(|d| d.sort_key());
    diff_list.dedup();
    let diff_badge = if diff_list.is_empty() {
        None
    } else {
        Some(view! {
            <div class="difficulty-badges">
                {diff_list.into_iter().map(|d| {
                    let css_class = format!("difficulty-badge diff-{}", d.css_class());
                    let label = d.label();
                    view! { <span class={css_class}>{label}</span> }
                }).collect_view()}
            </div>
        })
    };

    // Points display
    let points = data.points;
    let points_text = format!("{}pts", points);

    let ctf_title = data.ctf_title.clone();
    view! {
        <PageShell title=ctf_title.clone()>
            <header class="topbar">
                <div class="nav-buttons">
                    <a class="ctf-title" href="/">{ctf_title}</a>
                </div>
                <div class="nav-buttons">
                    <a class="btn secondary" href="/scoreboard">"Scoreboard"</a>
                    <span class="muted">{data.user_label.clone()}</span>
                    <form method="post" action="/logout" style="display:inline-block;">
                        <button class="btn secondary small" type="submit">"Logout"</button>
                    </form>
                </div>
            </header>

            <div class={detail_class}>
                <div class="detail-header">
                    <div class="title-row">
                        <h1>{full_name}</h1>
                        {diff_badge}
                        <span class="points-badge large">{points_text}</span>
                    </div>
                    {data.solved.then(|| view! { <span class="solved-badge large">"✓ Solved"</span> })}
                </div>

                <div class="detail-body">
                    <section class="description-section">
                        <h2>"Description"</h2>
                        <p>{description}</p>
                    </section>

                    <section class="files-section">
                        <h2>"Files"</h2>
                        {if files.is_empty() {
                            view! { <p class="muted">"No files for this challenge"</p> }.into_any()
                        } else {
                            view! {
                                <div class="files-list">
                                    {files.into_iter().map(|(href, label)| {
                                        view! { <a class="file-link" href={href} target="_blank">{label}</a> }
                                    }).collect_view()}
                                </div>
                            }.into_any()
                        }}
                    </section>
                </div>

                {(!data.solved).then(move || {
                    view! {
                        <div class="files-instance-sep"></div>
                        <div class="controls-wrapper">
                            <InstanceControls
                                instancing=data.instancing
                                has_instance=data.has_instance
                                ready=data.ready
                                can_extend=data.can_extend
                                challenge_name=challenge_name.clone()
                                submit_action=submit_action.to_string()
                                extend_display=extend_display.to_string()
                                    flags=flags.clone()
                                    flag_points=flag_points.clone()
                                submit_fb_flag_name=submit_fb_flag_name.clone()
                                submit_fb_kind=submit_fb_kind.clone()
                                submit_fb_message=submit_fb_message.clone()
                                exposed_ports=exposed_ports.clone()
                                instance_name=data.instance_name.clone()
                                instance_base_domain=data.instance_base_domain.clone()
                                instance_phase=data.instance_phase_desc.clone()
                                time_left_seconds=data.time_left_seconds
                                flag_hint=flag_hint.clone()
                            />
                        </div>
                    }
                })}
            </div>
            <script>{instance_script}</script>
        </PageShell>
    }
}
