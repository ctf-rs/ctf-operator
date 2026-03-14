use leptos::prelude::*;

use crate::helpers::challenge_route;

use super::{
    ChallengePageData, CategorySection,
    components::{ChallengeCard, InstanceControls},
    display_file_name, file_route, format_countdown,
    layout::PageShell,
    scripts::instance_live_script,
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
    view! {
        <PageShell title=ctf_title.clone()>
            <header class="topbar">
                <div>
                    <h1>{ctf_title}</h1>
                    <p class="muted">"Logged in as " {user_label}</p>
                </div>
                <form method="post" action="/logout">
                    <button class="btn secondary" type="submit">"Logout"</button>
                </form>
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
        </PageShell>
    }
}

// ---------------------------------------------------------------------------
// Challenge detail page
// ---------------------------------------------------------------------------

#[component]
pub fn ChallengeDetailPage(data: ChallengePageData) -> impl IntoView {
    let ch = &data.challenge;
    let tile_class = if data.solved { "tile solved" } else { "tile" };

    let base = challenge_route(&ch.name);
    let spawn_action = format!("{base}spawn");
    let extend_action = format!("{base}extend");
    let submit_action = format!("{base}submit");

    let full_name = ch.full_name.clone();
    let description = ch
        .description
        .clone()
        .unwrap_or_else(|| "No description".to_string());
    let ctf_name = ch.ctf_name.clone();
    let challenge_name = ch.name.clone();
    let flags = ch.flags.clone();

    let files: Vec<(String, String)> = ch
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| (file_route(&ch.name, i), display_file_name(f)))
        .collect();

    let status_text = if data.solved {
        "This challenge is solved. Instance is stopped and submissions are disabled.".to_string()
    } else if !data.has_instance {
        "No active instance. Spawn to start the challenge.".to_string()
    } else if !data.ready {
        if data.time_left_seconds == Some(0) {
            "Instance expired. Spawn a new one to continue.".to_string()
        } else {
            "Instance is starting. This page updates automatically when it is ready.".to_string()
        }
    } else {
        "Instance is ready.".to_string()
    };

    let timer_text = data
        .time_left_seconds
        .map(|s| format!("Time left: {}", format_countdown(s)))
        .unwrap_or_default();

    let extend_display = if data.can_extend { "block" } else { "none" };

    let instance_script = instance_live_script(&challenge_name);

    // Build feedback fragments
    let extend_fb = data.extend_feedback.clone();
    let submit_fb_flag_name = data.submit_feedback.as_ref().map(|f| f.flag_name.clone());
    let submit_fb_kind = data.submit_feedback.as_ref().map(|f| f.kind.clone());
    let submit_fb_message = data.submit_feedback.as_ref().map(|f| f.message.clone());

    view! {
        <PageShell title=data.ctf_title.clone()>
            <header class="topbar">
                <div>
                    <h1>{full_name.clone()}</h1>
                    <p class="muted">"Logged in as " {data.user_label.clone()}</p>
                </div>
                <div>
                    <a class="btn secondary" href="/">"Back"</a>
                    <form method="post" action="/logout" style="display:inline-block;margin-left:.5rem;">
                        <button class="btn secondary" type="submit">"Logout"</button>
                    </form>
                </div>
            </header>
            <div class="grid">
                <article class={tile_class}>
                    <h2>{full_name}</h2>
                    {data.solved.then(|| view! { <p class="solved-badge">"Challenge solved"</p> })}
                    <p class="muted">{description}</p>
                    <div class="meta">"CTF: " {ctf_name}</div>

                    <h3>"Files"</h3>
                    <ul>
                        {files.into_iter().map(|(href, label)| {
                            view! { <li><a href={href} target="_blank">{label}</a></li> }
                        }).collect_view()}
                    </ul>

                    <p id="instance-status" class="muted">{status_text}</p>
                    <p id="instance-timer" class="muted">{timer_text}</p>

                    {extend_fb.map(|msg| view! { <p>{msg}</p> })}

                    {(!data.solved).then(move || {
                        view! {
                            <InstanceControls
                                has_instance=data.has_instance
                                ready=data.ready
                                can_extend=data.can_extend
                                spawn_action=spawn_action.clone()
                                extend_action=extend_action.clone()
                                submit_action=submit_action.clone()
                                extend_display=extend_display.to_string()
                                flags=flags.clone()
                                submit_fb_flag_name=submit_fb_flag_name.clone()
                                submit_fb_kind=submit_fb_kind.clone()
                                submit_fb_message=submit_fb_message.clone()
                            />
                        }
                    })}
                </article>
            </div>
            <script>{instance_script}</script>
        </PageShell>
    }
}
