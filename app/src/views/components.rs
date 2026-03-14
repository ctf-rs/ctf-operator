use leptos::prelude::*;

use crate::helpers::challenge_route;

use super::ChallengeCardData;
use crate::db::{FlagView, PortView};

// ---------------------------------------------------------------------------
// Challenge card (used on index page grid)
// ---------------------------------------------------------------------------

#[component]
pub fn ChallengeCard(data: ChallengeCardData) -> impl IntoView {
    let ch = &data.challenge;
    let tile_class = if data.solved {
        "tile card solved"
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

    view! {
        <article class={tile_class}>
            <div class="card-content">
                <div class="tile-header">
                    <h2>{full_name}</h2>
                    {diff_badge}
                </div>
                {data.solved.then(|| view! { <p class="solved-badge">"Solved"</p> })}
                <p class="muted desc">{description}</p>
            </div>
            <a class="btn" href={route}>"Open challenge"</a>
        </article>
    }
}

// ---------------------------------------------------------------------------
// Instance controls (spawn / extend / flag forms)
// ---------------------------------------------------------------------------

#[allow(unused_variables)]
#[component]
pub fn InstanceControls(
    has_instance: bool,
    ready: bool,
    can_extend: bool,
    spawn_action: String,
    extend_action: String,
    submit_action: String,
    extend_display: String,
    flags: Vec<FlagView>,
    submit_fb_flag_name: Option<String>,
    submit_fb_kind: Option<String>,
    submit_fb_message: Option<String>,
    exposed_ports: Vec<PortView>,
    instance_name: Option<String>,
    instance_base_domain: Option<String>,
) -> impl IntoView {
    let extend_form = view! {
        <form id="extend-form" method="post" action={extend_action.clone()} style={format!("display:{extend_display}")}>
            <button class="btn secondary" type="submit">"Extend"</button>
        </form>
    };

    let spawn_btn = if !has_instance {
        view! {
            <form method="post" action={spawn_action.clone()}>
                <button class="btn primary" type="submit">"Spawn Instance"</button>
            </form>
        }
        .into_any()
    } else if !ready {
        view! {
            <form method="post" action={spawn_action.clone()}>
                <button class="btn" type="submit">"Spawn again"</button>
            </form>
        }
        .into_any()
    } else {
        view! { <div></div> }.into_any()
    };

    let flags_disabled = !ready;
    let flags_for_forms = flags.clone();

    // Generate connection details when instance is ready
    let connection_details = if ready && !exposed_ports.is_empty() {
        let base_domain = instance_base_domain.unwrap_or_else(|| "ctf.local".to_string());
        let inst_name = instance_name.unwrap_or_else(|| "instance".to_string());
        let host = format!("{inst_name}.{base_domain}");
        
        let connections: Vec<_> = exposed_ports
            .into_iter()
            .map(|port| {
                let conn_str = match port.port_type.as_str() {
                    "http" => format!("http://{host}:{}", port.port),
                    "https" => format!("https://{host}:{}", port.port),
                    "netcat" => format!("nc {host} {}", port.port),
                    "stty" => format!("stty raw -echo; nc {host} {}; stty sane", port.port),
                    "ssh" => format!("ssh user@{host} -p {}", port.port),
                    _ => format!("{host}:{}", port.port),
                };
                let is_link = matches!(port.port_type.as_str(), "http" | "https");
                (port.name, port.port_type, conn_str, is_link)
            })
            .collect();

        view! {
            <div class="connection-section">
                <h3>"Connection Details"</h3>
                {connections.into_iter().map(|(name, port_type, conn_str, is_link)| {
                    let label = format!("{} ({})", name, port_type);
                    let conn_display = conn_str.clone();
                    if is_link {
                        view! {
                            <div class="connection-item">
                                <div class="label">{label}</div>
                                <a href={conn_str} target="_blank"><code>{conn_display}</code></a>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class="connection-item">
                                <div class="label">{label}</div>
                                <code>{conn_str}</code>
                            </div>
                        }.into_any()
                    }
                }).collect_view()}
            </div>
        }
        .into_any()
    } else {
        view! { <div></div> }.into_any()
    };

    view! {
        <div class="controls-row">
            {spawn_btn}
            {extend_form}
        </div>
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
                view! {
                    <FlagForm
                        action=submit_action.clone()
                        flag_name=flag.name.clone()
                        flag_label=display_label
                        disabled=flags_disabled
                        feedback=feedback
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
    feedback: Option<(String, String)>,
) -> impl IntoView {
    let placeholder = format!("{flag_label} flag");
    let form_class = if disabled {
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
