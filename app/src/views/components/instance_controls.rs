use leptos::prelude::*;

use crate::db::{FlagView, PortView};
use crate::views::format_countdown;
use ctf_cli::ChallengeDifficulty;

// ---------------------------------------------------------------------------
// Instance controls (connections & flag forms)
// ---------------------------------------------------------------------------

#[allow(unused_variables)]
#[component]
pub fn InstanceControls(
    instancing: bool,
    has_instance: bool,
    ready: bool,
    can_extend: bool,
    challenge_name: String,
    submit_action: String,
    extend_display: String,
    flags: Vec<FlagView>,
    flag_points: Vec<u32>,
    submit_fb_flag_name: Option<String>,
    submit_fb_kind: Option<String>,
    submit_fb_message: Option<String>,
    exposed_ports: Vec<PortView>,
    instance_name: Option<String>,
    instance_base_domain: String,
    instance_phase: Option<String>,
    time_left_seconds: Option<u64>,
    flag_hint: String,
    /// When true the CTF has ended and flag submission is disabled.
    #[prop(optional)]
    ctf_ended: bool,
) -> impl IntoView {
    let flags_disabled = !ready || ctf_ended;
    let flags_for_forms = flags.clone();
    let points_for_forms = flag_points.clone();

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

    // Phase is always shown (far right); timer left of phase when instance has time remaining.
    // Controls (Spawn/Stop/Extend) depend on instance state.
    let instance_controls = if instancing {
        // Phase text: always visible
        let phase_text = instance_phase.clone().unwrap_or_else(|| {
            if has_instance {
                "Starting...".to_string()
            } else {
                "No instance".to_string()
            }
        });

        // Timer: only when instance exists and has time left
        let timer_view = if has_instance {
            time_left_seconds.map(|s| {
                let t = format_countdown(s);
                view! { <span class="timer-badge">{t}</span> }.into_any()
            })
        } else {
            None
        };

        // Control buttons depend on state
        let control_buttons = if has_instance && ready {
            // Show Stop and optionally Extend
            let extend_button = if can_extend {
                view! {
                    <form method="post" action="/api/extend_instance" style="display:inline-block;">
                        <input type="hidden" name="challenge_name" value={challenge_name.clone()}/>
                        <button class="btn extend" type="submit">"Extend"</button>
                    </form>
                }
                .into_any()
            } else {
                view! { <span></span> }.into_any()
            };

            view! {
                <>
                    <form method="post" action="/api/stop_instance" style="display:inline-block;">
                        <input type="hidden" name="challenge_name" value={challenge_name.clone()}/>
                        <button class="btn danger" type="submit">"Stop instance"</button>
                    </form>
                    {extend_button}
                </>
            }
            .into_any()
        } else if !has_instance {
            // Show Spawn button
            view! {
                <form method="post" action="/api/spawn_instance" style="display:inline-block;">
                    <input type="hidden" name="challenge_name" value={challenge_name.clone()}/>
                    <button class="btn primary" type="submit">"Spawn instance"</button>
                </form>
            }
            .into_any()
        } else {
            // has_instance but not ready: no controls, just status
            view! { <span></span> }.into_any()
        };

        view! {
            <div class="controls-row">
                <div class="controls-left">
                    {control_buttons}
                </div>
                <div class="controls-right">
                    {timer_view}
                    <span class="status-badge">{phase_text}</span>
                </div>
            </div>
        }
        .into_any()
    } else {
        view! { <span></span> }.into_any()
    };

    view! {
        {instance_controls}
        {connection_details}
        <div class="flags-section">
            <h3>"Submit Flags"</h3>
            {flags_for_forms.into_iter().zip(points_for_forms.into_iter()).map(|(flag, pts)| {
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
                        challenge_name=challenge_name.clone()
                        flag_name=flag.name.clone()
                        flag_label=display_label
                        flag_difficulty=flag.difficulty
                        flag_points=pts
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

// Helper: determine if instancing UI should be shown. We allow it when there is at least one flag form
fn instancing_allowed(flags: &Vec<crate::db::FlagView>) -> bool {
    // currently always allow; future checks (user perms) can be added here
    !flags.is_empty()
}

// ---------------------------------------------------------------------------
// Single flag submission form
// ---------------------------------------------------------------------------

#[component]
fn FlagForm(
    action: String,
    challenge_name: String,
    flag_name: String,
    flag_label: String,
    flag_difficulty: Option<ChallengeDifficulty>,
    flag_points: u32,
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
            <input type="hidden" name="challenge_name" value={challenge_name}/>
            <input type="hidden" name="flag_name" value={flag_name.clone()}/>
            <input type="text" name="submitted_flag" placeholder={placeholder} required disabled=disabled/>
            <button class="btn" type="submit" disabled=disabled>"Submit"</button>
            {flag_difficulty.map(|d| {
                let css = format!("difficulty-badge diff-{}", d.css_class());
                let label = d.label();
                view! { <span class={css}>{label}</span> }
            })}
            <span class="points-badge small">{format!("{}pts", flag_points)}</span>
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
