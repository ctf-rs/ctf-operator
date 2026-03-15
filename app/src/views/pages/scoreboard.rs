use leptos::prelude::*;

use super::super::{ScoreboardPageData, layout::PageShell};

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
