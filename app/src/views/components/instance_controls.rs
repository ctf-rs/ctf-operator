use leptos::prelude::*;

use crate::db::{FlagView, PortView};

// ---------------------------------------------------------------------------
// Instance controls (connections & flag forms)
// ---------------------------------------------------------------------------

#[allow(unused_variables)]
#[component]
pub fn InstanceControls(
    has_instance: bool,
    ready: bool,
    can_extend: bool,
    challenge_name: String,
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
                        challenge_name=challenge_name.clone()
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
    challenge_name: String,
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
            <input type="hidden" name="challenge_name" value={challenge_name}/>
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
