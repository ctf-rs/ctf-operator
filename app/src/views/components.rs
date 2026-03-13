use leptos::prelude::*;

use crate::helpers::challenge_route;

use super::{ChallengeCardData, display_file_name, file_route};

// ---------------------------------------------------------------------------
// Challenge card (used on index page grid)
// ---------------------------------------------------------------------------

#[component]
pub fn ChallengeCard(data: ChallengeCardData) -> impl IntoView {
    let ch = &data.challenge;
    let tile_class = if data.solved { "tile solved" } else { "tile" };
    let route = challenge_route(&ch.name);
    let full_name = ch.full_name.clone();
    let description = ch
        .description
        .clone()
        .unwrap_or_else(|| "No description".to_string());
    let ctf_name = ch.ctf_name.clone();
    let files: Vec<(String, String)> = ch
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| (file_route(&ch.name, i), display_file_name(f)))
        .collect();

    view! {
        <article class={tile_class}>
            <h2>{full_name}</h2>
            {data.solved.then(|| view! { <p class="solved-badge">"Solved"</p> })}
            <p class="muted">{description}</p>
            <div class="meta">"CTF: " {ctf_name}</div>
            <h3>"Files"</h3>
            <ul>
                {files.into_iter().map(|(href, label)| {
                    view! { <li><a href={href} target="_blank">{label}</a></li> }
                }).collect_view()}
            </ul>
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
    flags: Vec<String>,
    submit_fb_flag_name: Option<String>,
    submit_fb_kind: Option<String>,
    submit_fb_message: Option<String>,
) -> impl IntoView {
    let extend_form = view! {
        <form id="extend-form" method="post" action={extend_action.clone()} style={format!("display:{extend_display}")}>
            <button class="btn secondary" type="submit">"Extend"</button>
        </form>
    };

    if !has_instance {
        return view! {
            <form method="post" action={spawn_action}>
                <button class="btn" type="submit">"Spawn"</button>
            </form>
        }
        .into_any();
    }

    if !ready {
        return view! {
            <form method="post" action={spawn_action}>
                <button class="btn" type="submit">"Spawn again"</button>
            </form>
            {extend_form}
        }
        .into_any();
    }

    view! {
        {extend_form}
        <h3>"Submit flags"</h3>
        {flags.into_iter().map(|flag_name| {
            let feedback = if submit_fb_flag_name.as_deref() == Some(&flag_name) {
                Some((submit_fb_kind.clone().unwrap_or_default(), submit_fb_message.clone().unwrap_or_default()))
            } else {
                None
            };
            view! {
                <FlagForm
                    action=submit_action.clone()
                    flag_name=flag_name
                    feedback=feedback
                />
            }
        }).collect_view()}
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
    feedback: Option<(String, String)>,
) -> impl IntoView {
    let placeholder = format!("{flag_name} flag");
    view! {
        <form method="post" action={action} class="flag-form">
            <input type="hidden" name="flag_name" value={flag_name.clone()}/>
            <input type="text" name="submitted_flag" placeholder={placeholder} required/>
            <button class="btn" type="submit">"Submit"</button>
        </form>
        {feedback.map(|(kind, message)| {
            if kind == "good" {
                view! { <p><strong>"Good flag: "</strong> {flag_name.clone()} " accepted. " {message}</p> }.into_any()
            } else {
                view! { <p><strong>"Bad flag: "</strong> {flag_name.clone()} ". " {message}</p> }.into_any()
            }
        })}
    }
}
