use leptos::prelude::*;

use super::super::layout::PageShell;

// ---------------------------------------------------------------------------
// Access denied page (non-public CTF, user lacks role)
// ---------------------------------------------------------------------------

#[component]
pub fn AccessDeniedPage(ctf_title: String, user_label: String) -> impl IntoView {
    view! {
        <PageShell title=ctf_title.clone()>
            <header class="topbar">
                <div class="nav-left">
                    <a class="ctf-title" href="/">{ctf_title.clone()}</a>
                </div>
                <div class="nav-center"></div>
                <div class="nav-right">
                    <span class="muted">{user_label}</span>
                    <form method="post" action="/logout" style="display:inline-block;">
                        <button class="btn secondary small" type="submit">"Logout"</button>
                    </form>
                </div>
            </header>

            <div style="text-align:center;margin-top:6rem;">
                <h1>"Access Denied"</h1>
                <p class="muted" style="margin-top:1.5rem;">
                    "You do not have permission to access " {ctf_title} "."
                </p>
                <p class="muted" style="margin-top:0.5rem;">
                    "If you believe this is an error, please contact an organiser."
                </p>
            </div>
        </PageShell>
    }
}

// ---------------------------------------------------------------------------
// CTF not-yet-open page (with optional countdown timer)
// ---------------------------------------------------------------------------

#[component]
pub fn CtfNotOpenPage(
    ctf_title: String,
    user_label: String,
    /// Seconds until the CTF opens (`None` when no open_time is set).
    seconds_until_open: Option<u64>,
) -> impl IntoView {
    let countdown_script = seconds_until_open.map(|secs| {
        format!(
            r#"(function(){{
                let left={secs};
                const el=document.getElementById('countdown');
                function fmt(s){{
                    const d=Math.floor(s/86400);
                    const h=Math.floor((s%86400)/3600);
                    const m=Math.floor((s%3600)/60);
                    const sec=s%60;
                    let parts=[];
                    if(d>0)parts.push(d+'d');
                    if(h>0)parts.push(h+'h');
                    if(m>0)parts.push(m+'m');
                    parts.push(sec+'s');
                    return parts.join(' ');
                }}
                function tick(){{
                    if(left<=0){{window.location.reload();return;}}
                    left--;
                    if(el)el.textContent=fmt(left);
                }}
                if(el)el.textContent=fmt(left);
                setInterval(tick,1000);
            }})()"#,
        )
    });

    view! {
        <PageShell title=ctf_title.clone()>
            <header class="topbar">
                <div class="nav-left">
                    <a class="ctf-title" href="/">{ctf_title.clone()}</a>
                </div>
                <div class="nav-center"></div>
                <div class="nav-right">
                    <span class="muted">{user_label}</span>
                    <form method="post" action="/logout" style="display:inline-block;">
                        <button class="btn secondary small" type="submit">"Logout"</button>
                    </form>
                </div>
            </header>

            <div class="not-open-container" style="text-align:center;margin-top:6rem;">
                <h1>{ctf_title} " has not started yet"</h1>
                {seconds_until_open.map(|_| view! {
                    <p style="font-size:1.25rem;margin-top:1.5rem;">"Opens in"</p>
                    <p id="countdown" class="countdown-timer" style="font-size:2.5rem;font-weight:bold;margin-top:0.5rem;font-variant-numeric:tabular-nums;">
                        "..."
                    </p>
                })}
                {seconds_until_open.is_none().then(|| view! {
                    <p class="muted" style="margin-top:1.5rem;">"Check back later."</p>
                })}
            </div>
            {countdown_script.map(|s| view! { <script>{s}</script> })}
        </PageShell>
    }
}

// ---------------------------------------------------------------------------
// No-team page (team mode, user is not on a team)
// ---------------------------------------------------------------------------

#[component]
pub fn NoTeamPage(ctf_title: String, user_label: String) -> impl IntoView {
    view! {
        <PageShell title=ctf_title.clone()>
            <header class="topbar">
                <div class="nav-left">
                    <a class="ctf-title" href="/">{ctf_title.clone()}</a>
                </div>
                <div class="nav-center"></div>
                <div class="nav-right">
                    <span class="muted">{user_label}</span>
                    <form method="post" action="/logout" style="display:inline-block;">
                        <button class="btn secondary small" type="submit">"Logout"</button>
                    </form>
                </div>
            </header>

            <div style="text-align:center;margin-top:6rem;">
                <h1>"No Team Assigned"</h1>
                <p class="muted" style="margin-top:1.5rem;">
                    "This CTF requires team membership. You are not currently assigned to any team."
                </p>
                <p class="muted" style="margin-top:0.5rem;">
                    "Please contact an organiser to get assigned to a team."
                </p>
            </div>
        </PageShell>
    }
}
