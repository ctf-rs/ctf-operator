use leptos::prelude::*;

use crate::helpers::challenge_route;

use super::{
    CategorySection, ChallengePageData, ScoreboardPageData,
    components::{ChallengeCard, InstanceControls},
    display_file_name, file_route, format_countdown,
    layout::PageShell,
    scripts::{index_timer_script, instance_live_script},
};

// ---------------------------------------------------------------------------
// Login page
// ---------------------------------------------------------------------------

#[component]
pub fn LoginPage(ctf_title: String) -> impl IntoView {
    view! {
        <PageShell title=ctf_title.clone()>
            <div class="auth-card">
                <h1>{ctf_title}</h1>
                <p>"Login required to view challenges."</p>
                <a class="btn" href="/login">"Login with Keycloak"</a>
            </div>
        </PageShell>
    }
}

// ---------------------------------------------------------------------------
// Challenge list (index page)
// ---------------------------------------------------------------------------

#[component]
pub fn IndexPage(
    ctf_title: String,
    user_label: String,
    sections: Vec<CategorySection>,
) -> impl IntoView {
    let timer_script = index_timer_script();
    view! {
        <PageShell title=ctf_title.clone()>
            <header class="topbar">
                <div>
                    <h1>{ctf_title}</h1>
                    <p class="muted">"Logged in as " {user_label}</p>
                </div>
                <div class="nav-buttons">
                    <a class="btn secondary" href="/scoreboard">"Scoreboard"</a>
                    <form method="post" action="/logout" style="display:inline-block;">
                        <button class="btn secondary" type="submit">"Logout"</button>
                    </form>
                </div>
            </header>
            {sections.into_iter().map(|section| {
                let label = section.label.clone();
                view! {
                    <section class="category-section">
                        <h2 class="category-heading">{label}</h2>
                        <div class="grid">
                            {section.cards.into_iter().map(|card| view! { <ChallengeCard data=card/> }).collect_view()}
                        </div>
                    </section>
                }
            }).collect_view()}
            <script>{timer_script}</script>
        </PageShell>
    }
}

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

    let base = challenge_route(&ch.name);
    let spawn_action = format!("{base}spawn");
    let extend_action = format!("{base}extend");
    let stop_action = format!("{base}stop");
    let submit_action = format!("{base}submit");

    let full_name = ch.full_name.clone();
    let description = ch
        .description
        .clone()
        .unwrap_or_else(|| "No description".to_string());
    let challenge_name = ch.name.clone();
    let flags = ch.flags.clone();
    let difficulty = ch.difficulty;
    let exposed_ports = ch.exposed_ports.clone();
    let flag_hint = ch.flag_hint.clone();

    let files: Vec<(String, String)> = ch
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| (file_route(&ch.name, i), display_file_name(f)))
        .collect();

    let status_text = if data.solved {
        "Solved".to_string()
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

    // Only show timer when instance is ready and has time left
    let show_timer = data.ready && data.time_left_seconds.is_some();
    let timer_text = data
        .time_left_seconds
        .map(|s| format_countdown(s))
        .unwrap_or_default();

    let extend_display = if data.can_extend {
        "inline-block"
    } else {
        "none"
    };

    let instance_script = instance_live_script(&challenge_name);

    let extend_fb = data.extend_feedback.clone();
    let submit_fb_flag_name = data.submit_feedback.as_ref().map(|f| f.flag_name.clone());
    let submit_fb_kind = data.submit_feedback.as_ref().map(|f| f.kind.clone());
    let submit_fb_message = data.submit_feedback.as_ref().map(|f| f.message.clone());

    let diff_badge = difficulty.map(|d| {
        let css_class = format!("difficulty-badge diff-{}", d.css_class());
        let label = d.label();
        view! { <span class={css_class}>{label}</span> }
    });

    // Points display
    let points = data.points;
    let points_text = format!("{}pts", points);

    view! {
        <PageShell title=data.ctf_title.clone()>
            <header class="topbar">
                <div class="nav-buttons">
                    <a class="btn secondary" href="/">"← Challenges"</a>
                    <a class="btn secondary" href="/scoreboard">"Scoreboard"</a>
                </div>
                <div class="nav-buttons">
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

                <div class="instance-panel">
                    <div class="instance-header">
                        <h2>"Instance"</h2>
                        <div class="instance-actions" id="instance-controls">
                            {(!data.solved && !data.deleting && (!data.has_instance || data.time_left_seconds == Some(0))).then(|| view! {
                                <form method="post" action={spawn_action.clone()} id="spawn-form">
                                    <button class="btn primary small" type="submit" id="spawn-btn">"Spawn"</button>
                                </form>
                            })}
                            {(!data.solved && data.has_instance && !data.deleting).then(|| view! {
                                <form method="post" action={stop_action.clone()} id="stop-form">
                                    <button class="btn danger small" type="submit" id="stop-btn">"Stop"</button>
                                </form>
                            })}
                            {(!data.solved && data.has_instance && !data.deleting).then(|| {
                                let extend_style = if data.can_extend { "" } else { "display:none" };
                                view! {
                                    <form method="post" action={extend_action.clone()} id="extend-form" style={extend_style}>
                                        <button class="btn extend small" type="submit" id="extend-btn">"Extend"</button>
                                    </form>
                                }
                            })}
                        </div>
                        {extend_fb.map(|msg| view! { <span class="feedback-inline">{msg}</span> })}
                        <div class="instance-meta">
                            <span id="instance-status" class={if data.ready { "status-badge ready" } else if data.deleting { "status-badge deleting" } else { "status-badge" }}>{status_text}</span>
                            {show_timer.then(|| view! { <span id="instance-timer" class="timer-badge">{timer_text}</span> })}
                        </div>
                    </div>

                    {(!data.solved).then(move || {
                        view! {
                            <InstanceControls
                                has_instance=data.has_instance
                                ready=data.ready
                                can_extend=data.can_extend
                                spawn_action=spawn_action.clone()
                                extend_action=extend_action.clone()
                                stop_action=stop_action.clone()
                                submit_action=submit_action.clone()
                                extend_display=extend_display.to_string()
                                flags=flags.clone()
                                submit_fb_flag_name=submit_fb_flag_name.clone()
                                submit_fb_kind=submit_fb_kind.clone()
                                submit_fb_message=submit_fb_message.clone()
                                exposed_ports=exposed_ports.clone()
                                instance_name=data.instance_name.clone()
                                instance_base_domain=data.instance_base_domain.clone()
                                flag_hint=flag_hint.clone()
                            />
                        }
                    })}
                </div>
            </div>
            <script>{instance_script}</script>
        </PageShell>
    }
}

// ---------------------------------------------------------------------------
// Scoreboard page
// ---------------------------------------------------------------------------

#[component]
pub fn ScoreboardPage(data: ScoreboardPageData) -> impl IntoView {
    view! {
        <PageShell title=format!("{} - Scoreboard", data.ctf_title)>
            <header class="topbar">
                <div class="nav-buttons">
                    <a class="btn secondary" href="/">"← Challenges"</a>
                </div>
                <div class="nav-buttons">
                    <span class="muted">{data.user_label.clone()}</span>
                    <form method="post" action="/logout" style="display:inline-block;">
                        <button class="btn secondary small" type="submit">"Logout"</button>
                    </form>
                </div>
            </header>

            <div class="scoreboard-container">
                <h1>"Scoreboard"</h1>
                {if data.leaderboard.is_empty() {
                    view! {
                        <p class="muted" style="text-align:center;margin-top:2rem;">"No solves yet. Be the first!"</p>
                    }.into_any()
                } else {
                    view! {
                        <table class="scoreboard-table">
                            <thead>
                                <tr>
                                    <th class="rank">"#"</th>
                                    <th class="username">"Player"</th>
                                    <th class="points">"Points"</th>
                                    <th class="solves">"Solves"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {data.leaderboard.into_iter().enumerate().map(|(i, entry)| {
                                    let rank = i + 1;
                                    let rank_class = match rank {
                                        1 => "rank gold",
                                        2 => "rank silver",
                                        3 => "rank bronze",
                                        _ => "rank",
                                    };
                                    view! {
                                        <tr>
                                            <td class={rank_class}>{rank}</td>
                                            <td class="username">{entry.username}</td>
                                            <td class="points">{entry.total_points}</td>
                                            <td class="solves">{entry.solves}</td>
                                        </tr>
                                    }
                                }).collect_view()}
                            </tbody>
                        </table>
                    }.into_any()
                }}
            </div>
        </PageShell>
    }
}
