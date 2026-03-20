use leptos::prelude::*;

use super::super::{ScoreboardPageData, layout::PageShell};

// ---------------------------------------------------------------------------
// Scoreboard page
// ---------------------------------------------------------------------------

#[component]
pub fn ScoreboardPage(data: ScoreboardPageData) -> impl IntoView {
    let ctf_title = data.ctf_title.clone();
    view! {
        <PageShell title=format!("{} - Scoreboard", ctf_title.clone())>
            <header class="topbar">
                <div class="nav-left">
                    <a class="ctf-title" href="/">{ctf_title}</a>
                </div>
                <div class="nav-center"></div>
                <div class="nav-right">
                    <span class="muted">{data.user_label.clone()}</span>
                    <form method="post" action="/logout" style="display:inline-block;">
                        <button class="btn secondary small" type="submit">"Logout"</button>
                    </form>
                </div>
            </header>

            <div class="scoreboard-container">
                <h1>"Scoreboard"</h1>
                {data.frozen.then(|| view! {
                    <p class="frozen-banner" style="text-align:center;color:#93c5fd;font-weight:600;margin:0.5rem 0 1rem;">
                        "\u{2744}\u{fe0f} Scoreboard is frozen — scores may not reflect latest solves."
                    </p>
                })}
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

// ---------------------------------------------------------------------------
// Scoreboard hidden page
// ---------------------------------------------------------------------------

#[component]
pub fn ScoreboardHiddenPage(ctf_title: String, user_label: String) -> impl IntoView {
    view! {
        <PageShell title=format!("{} - Scoreboard", ctf_title.clone())>
            <header class="topbar">
                <div class="nav-left">
                    <a class="ctf-title" href="/">{ctf_title}</a>
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
                <h1>"Scoreboard"</h1>
                <p class="muted" style="margin-top:1.5rem;">"The scoreboard is currently hidden."</p>
            </div>
        </PageShell>
    }
}
