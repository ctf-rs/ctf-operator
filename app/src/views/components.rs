use leptos::prelude::*;

use crate::helpers::challenge_route;

use super::{ChallengeCardData, format_countdown};
use crate::db::{FlagView, PortView};

// ---------------------------------------------------------------------------
// Challenge card (used on index page grid)
// ---------------------------------------------------------------------------

#[component]
pub fn ChallengeCard(data: ChallengeCardData) -> impl IntoView {
    let ch = &data.challenge;
    let tile_class = if data.solved {
        "tile card solved"
    } else if data.partial_solve {
        "tile card partial"
    } else {
        "tile card"
    };
    let route = challenge_route(&ch.name);
    let full_name = ch.full_name.clone();
    let description = ch
        .description
        .clone()
        .unwrap_or_else(|| "No description".to_string());
    let difficulty = ch.difficulty;

    let diff_badge = difficulty.map(|d| {
        let css_class = format!("difficulty-badge diff-{}", d.css_class());
        let label = d.label();
        view! { <span class={css_class}>{label}</span> }
    });

    let status_badge = if data.solved {
        Some(view! { <p class="solved-badge">"Solved"</p> }.into_any())
    } else if data.partial_solve {
        Some(view! { <p class="partial-badge">"In Progress"</p> }.into_any())
    } else {
        None
    };

    // Flag progress indicator (N/M)
    let flag_progress = format!("{}/{}", data.solved_flags, data.total_flags);
    let flag_progress_class = if data.solved {
        "flag-progress completed"
    } else if data.partial_solve {
        "flag-progress partial"
    } else {
        "flag-progress"
    };

    // Points badge
    let points = data.points;
    let points_display = format!("{}pts", points);

    // Instance timer (only show if has instance and time remaining)
    let timer_badge = if data.has_instance {
        if let Some(secs) = data.time_left_seconds {
            Some(view! { <span class="card-timer">{format_countdown(secs)}</span> }.into_any())
        } else {
            Some(view! { <span class="card-timer running">"Running"</span> }.into_any())
        }
    } else {
        None
    };

    view! {
        <article class={tile_class}>
            <div class="card-content">
                <div class="tile-header">
                    <span class="tile-title">{full_name}</span>
                    {diff_badge}
                    {timer_badge}
                    <span class="points-badge">{points_display}</span>
                    <span class={flag_progress_class}>{flag_progress}</span>
                </div>
                {status_badge}
                <p class="muted desc">{description}</p>
            </div>
            <a class="btn" href={route}>"Open challenge"</a>
        </article>
    }
}

// ---------------------------------------------------------------------------
// Instance controls (connections & flag forms)
// ---------------------------------------------------------------------------

#[allow(unused_variables)]
#[component]
pub fn InstanceControls(
    has_instance: bool,
    ready: bool,
    can_extend: bool,
    spawn_action: String,
    extend_action: String,
    stop_action: String,
    submit_action: String,
    extend_display: String,
    flags: Vec<FlagView>,
    submit_fb_flag_name: Option<String>,
    submit_fb_kind: Option<String>,
    submit_fb_message: Option<String>,
    exposed_ports: Vec<PortView>,
    instance_name: Option<String>,
    instance_base_domain: String,
    flag_hint: String,
) -> impl IntoView {
    let flags_disabled = !ready;
    let flags_for_forms = flags.clone();

    // Generate connection details when instance is ready
    let connection_details = if ready && !exposed_ports.is_empty() {
        let base_domain = &instance_base_domain;

        let connections: Vec<_> = exposed_ports
            .into_iter()
            .map(|port| {
                // Use allocated_port if available, otherwise fall back to container port
                let actual_port = port.allocated_port.unwrap_or(port.port);
                let conn_str = match port.port_type.as_str() {
                    "http" => format!("http://{base_domain}:{actual_port}"),
                    "https" => format!("https://{base_domain}:{actual_port}"),
                    "netcat" => format!("nc {base_domain} {actual_port}"),
                    "stty" => format!("stty raw -echo; nc {base_domain} {actual_port}; stty sane"),
                    "ssh" => format!("ssh user@{base_domain} -p {actual_port}"),
                    _ => format!("{base_domain}:{actual_port}"),
                };
                let is_link = matches!(port.port_type.as_str(), "http" | "https");
                (conn_str, is_link)
            })
            .collect();

        view! {
            <div class="connection-section">
                <h3>"Connection Details"</h3>
                <div class="connection-list">
                    {connections.into_iter().map(|(conn_str, is_link)| {
                        let conn_display = conn_str.clone();
                        if is_link {
                            view! {
                                <div class="connection-item">
                                    <a href={conn_str} target="_blank"><code>{conn_display}</code></a>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="connection-item">
                                    <code>{conn_str}</code>
                                </div>
                            }.into_any()
                        }
                    }).collect_view()}
                </div>
            </div>
        }
        .into_any()
    } else {
        view! { <div></div> }.into_any()
    };

    view! {
        {connection_details}
        <div class="flags-section">
            <h3>"Submit Flags"</h3>
            {flags_for_forms.into_iter().map(|flag| {
                let feedback = if submit_fb_flag_name.as_deref() == Some(&flag.name) {
                    Some((submit_fb_kind.clone().unwrap_or_default(), submit_fb_message.clone().unwrap_or_default()))
                } else {
                    None
                };
                let display_label = flag.label.clone().unwrap_or_else(|| flag.name.clone());
                let flag_solved = flag.solved;
                view! {
                    <FlagForm
                        action=submit_action.clone()
                        flag_name=flag.name.clone()
                        flag_label=display_label
                        disabled=flags_disabled || flag_solved
                        solved=flag_solved
                        feedback=feedback
                        flag_hint=flag_hint.clone()
                    />
                }
            }).collect_view()}
        </div>
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Single flag submission form
// ---------------------------------------------------------------------------

#[component]
fn FlagForm(
    action: String,
    flag_name: String,
    flag_label: String,
    disabled: bool,
    solved: bool,
    feedback: Option<(String, String)>,
    flag_hint: String,
) -> impl IntoView {
    let placeholder = if solved {
        format!("✓ {flag_label} solved")
    } else {
        flag_hint.clone()
    };
    let form_class = if solved {
        "flag-form solved"
    } else if disabled {
        "flag-form disabled"
    } else {
        "flag-form"
    };
    view! {
        <form method="post" action={action} class={form_class}>
            <label class="flag-label">{flag_label.clone()}</label>
            <input type="hidden" name="flag_name" value={flag_name.clone()}/>
            <input type="text" name="submitted_flag" placeholder={placeholder} required disabled=disabled/>
            <button class="btn" type="submit" disabled=disabled>"Submit"</button>
        </form>
        {feedback.map(|(kind, message)| {
            if kind == "good" {
                view! { <p class="feedback good"><strong>"✓ "</strong> {flag_label.clone()} " accepted. " {message}</p> }.into_any()
            } else {
                view! { <p class="feedback bad"><strong>"✗ "</strong> {flag_label.clone()} ": " {message}</p> }.into_any()
            }
        })}
    }
}
