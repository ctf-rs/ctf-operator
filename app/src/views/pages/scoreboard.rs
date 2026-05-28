use leptos::prelude::*;

use super::super::{ScoreboardPageData, SolveStatus, layout::PageShell};

// ---------------------------------------------------------------------------
// Scoreboard page
// ---------------------------------------------------------------------------

#[component]
pub fn ScoreboardPage(data: ScoreboardPageData) -> impl IntoView {
    let ctf_title = data.ctf_title.clone();
    let player_header = if data.has_teams { "Team / Player" } else { "Player" };

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
                    let columns = data.challenge_columns.clone();
                    view! {
                        <div class="scoreboard-scroll">
                            <table class="scoreboard-table grid-table">
                                <thead>
                                    <tr>
                                        <th class="rank sticky-col">"#"</th>
                                        <th class="username sticky-col">{player_header}</th>
                                        <th class="points sticky-col">"Score"</th>
                                        {columns.iter().map(|ch| {
                                            let cat = ch.category.label();
                                            let cat_class = format!("challenge-col cat-{}", ch.category.sort_key());
                                            let title = format!("{} ({}pts)", ch.full_name, ch.points);
                                            view! {
                                                <th class={cat_class} title={title}>
                                                    <div class="rotated-header">
                                                        <span class="ch-name">{ch.full_name.clone()}</span>
                                                        <span class="ch-cat">{cat}</span>
                                                    </div>
                                                </th>
                                            }
                                        }).collect_view()}
                                    </tr>
                                </thead>
                                <tbody>
                                    {data.leaderboard.into_iter().zip(data.solve_grid.into_iter()).enumerate().map(|(i, (entry, solves))| {
                                        let rank = i + 1;
                                        let rank_class = match rank {
                                            1 => "rank gold",
                                            2 => "rank silver",
                                            3 => "rank bronze",
                                            _ => "rank",
                                        };
                                        view! {
                                            <tr>
                                                <td class={format!("{rank_class} sticky-col")}>{rank}</td>
                                                <td class="username sticky-col">{entry.username}</td>
                                                <td class="points sticky-col">{entry.total_points}</td>
                                                {solves.into_iter().enumerate().map(|(j, status)| {
                                                    let cat_class = format!("solve-cell cat-{}", columns[j].category.sort_key());
                                                    match status {
                                                        SolveStatus::FirstBlood => view! {
                                                            <td class={format!("{cat_class} first-blood")} title="First blood — first team/player to solve">{"\u{1f6a9}"}</td>
                                                        }.into_any(),
                                                        SolveStatus::Solved => view! {
                                                            <td class={format!("{cat_class} solved")} title="Solved">{"\u{2714}"}</td>
                                                        }.into_any(),
                                                        SolveStatus::Unsolved => view! {
                                                            <td class={cat_class}></td>
                                                        }.into_any(),
                                                    }
                                                }).collect_view()}
                                            </tr>
                                        }
                                    }).collect_view()}
                                </tbody>
                            </table>
                        </div>
                        <div class="scoreboard-legend">
                            <span class="legend-item"><span class="legend-icon first-blood">{"\u{1f6a9}"}</span>" First blood (first to solve)"</span>
                            <span class="legend-item"><span class="legend-icon solved">{"\u{2714}"}</span>" Solved"</span>
                        </div>
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
