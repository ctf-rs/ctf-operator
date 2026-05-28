use leptos::prelude::*;

use super::super::{ScoreboardPageData, ScoreboardTimer, SolveStatus, layout::PageShell};

/// Format seconds into a human-readable string like "2h 15m" or "45m 30s".
fn format_duration(total_secs: u64) -> String {
    let d = total_secs / 86400;
    let h = (total_secs % 86400) / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

// ---------------------------------------------------------------------------
// Timer progress bar component
// ---------------------------------------------------------------------------

#[component]
fn TimerBar(timer: ScoreboardTimer) -> impl IntoView {
    let total = timer.end_ts.saturating_sub(timer.start_ts);
    if total == 0 {
        return view! { <div></div> }.into_any();
    }

    let elapsed = timer.now_ts.saturating_sub(timer.start_ts).min(total);
    let progress_pct = (elapsed as f64 / total as f64 * 100.0).min(100.0);
    let remaining = timer.end_ts.saturating_sub(timer.now_ts);

    let freeze_pct = timer.freeze_ts.map(|ft| {
        let freeze_offset = ft.saturating_sub(timer.start_ts).min(total);
        (freeze_offset as f64 / total as f64 * 100.0).min(100.0)
    });

    let is_frozen = timer.freeze_ts.is_some_and(|ft| timer.now_ts >= ft);
    let is_ended = timer.now_ts >= timer.end_ts;

    let status_text = if is_ended {
        "CTF has ended".to_string()
    } else if is_frozen {
        format!("Scoreboard frozen — {} remaining", format_duration(remaining))
    } else {
        format!("{} remaining", format_duration(remaining))
    };

    // Progress bar color: green up to freeze point, yellow/orange after
    let bar_style = if let Some(fp) = freeze_pct {
        if progress_pct <= fp {
            // Haven't reached freeze point yet — all green
            format!("width:{progress_pct:.2}%;background:linear-gradient(90deg,#22c55e,#4ade80);")
        } else {
            // Past freeze point — green up to freeze, amber after
            format!(
                "width:{progress_pct:.2}%;background:linear-gradient(90deg,#22c55e 0%,#4ade80 {fp:.2}%,#f59e0b {fp:.2}%,#ef4444 100%);",
            )
        }
    } else {
        format!("width:{progress_pct:.2}%;background:linear-gradient(90deg,#22c55e,#4ade80);")
    };

    view! {
        <div class="timer-bar-container"
             data-start-ts={timer.start_ts.to_string()}
             data-end-ts={timer.end_ts.to_string()}
             data-freeze-ts={timer.freeze_ts.map(|f| f.to_string()).unwrap_or_default()}>
            <div class="timer-bar-header">
                <span class="timer-status">{status_text}</span>
            </div>
            <div class="timer-bar-track">
                <div class="timer-bar-fill" style={bar_style}></div>
                {freeze_pct.map(|fp| view! {
                    <div class="timer-bar-freeze-marker" style={format!("left:{fp:.2}%;")}
                         title="Scoreboard freeze point">
                        <span class="freeze-label">{"\u{2744}\u{fe0f}"}</span>
                    </div>
                })}
            </div>
        </div>
    }.into_any()
}

// ---------------------------------------------------------------------------
// Scoreboard page
// ---------------------------------------------------------------------------

#[component]
pub fn ScoreboardPage(data: ScoreboardPageData) -> impl IntoView {
    let ctf_title = data.ctf_title.clone();
    let player_header = if data.has_teams { "Team" } else { "Player" };

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
                {data.timer.map(|timer| view! { <TimerBar timer=timer/> })}
                {if data.leaderboard.is_empty() {
                    view! {
                        <p class="muted" style="text-align:center;margin-top:2rem;">"No solves yet. Be the first!"</p>
                    }.into_any()
                } else {
                    let columns = data.challenge_columns.clone();
                    view! {
                            <table class="scoreboard-table">
                                <thead>
                                    <tr>
                                        <th class="rank">"#"</th>
                                        <th class="username">{player_header}</th>
                                        <th class="points">"Score"</th>
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
                                                <td class={format!("{rank_class}")}>{rank}</td>
                                                <td class="username">{entry.username}</td>
                                                <td class="points">{entry.total_points}</td>
                                                {solves.into_iter().enumerate().map(|(j, status)| {
                                                    let cat_class = format!("solve-cell cat-{}", columns[j].category.sort_key());
                                                    match status {
                                                        SolveStatus::FirstBlood => view! {
                                                            <td class={format!("{cat_class} first-blood")} title="First blood — first team/player to solve">{"\u{1fa78}"}</td>
                                                        }.into_any(),
                                                        SolveStatus::Solved => view! {
                                                            <td class={format!("{cat_class} solved")} title="Solved">{"\u{1f6a9}"}</td>
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
                        <div class="scoreboard-legend">
                            <span class="legend-item"><span class="legend-icon first-blood">{"\u{1fa78}"}</span>" First blood (first to solve)"</span>
                            <span class="legend-item"><span class="legend-icon solved">{"\u{1f6a9}"}</span>" Solved"</span>
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
