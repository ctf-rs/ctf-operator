use leptos::prelude::*;

use crate::helpers::challenge_route;

use crate::views::{ChallengeCardData, format_countdown};

// ---------------------------------------------------------------------------
// Challenge card (used on index page grid)
// ---------------------------------------------------------------------------

#[component]
pub fn ChallengeCard(data: ChallengeCardData) -> impl IntoView {
    let ch = &data.challenge;
    let tile_class = if data.solved {
        "tile card solved"
    } else if data.partial_solve {
        "tile card partial"
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

    let status_badge = if data.solved {
        Some(view! { <p class="solved-badge">"Solved"</p> }.into_any())
    } else if data.partial_solve {
        Some(view! { <p class="partial-badge">"In Progress"</p> }.into_any())
    } else {
        None
    };

    // Flag progress indicator (N/M)
    let flag_progress = format!("{}/{}", data.solved_flags, data.total_flags);
    let flag_progress_class = if data.solved {
        "flag-progress completed"
    } else if data.partial_solve {
        "flag-progress partial"
    } else {
        "flag-progress"
    };

    // Points badge
    let points = data.points;
    let points_display = format!("{}pts", points);

    // Instance timer (only show if has instance and time remaining)
    let timer_badge = if data.has_instance {
        if let Some(secs) = data.time_left_seconds {
            Some(view! { <span class="card-timer">{format_countdown(secs)}</span> }.into_any())
        } else {
            Some(view! { <span class="card-timer running">"Running"</span> }.into_any())
        }
    } else {
        None
    };

    view! {
        <article class={tile_class}>
            <div class="card-content">
                <div class="tile-header">
                    <span class="tile-title">{full_name}</span>
                    {diff_badge}
                    {timer_badge}
                    <span class="points-badge">{points_display}</span>
                    <span class={flag_progress_class}>{flag_progress}</span>
                </div>
                {status_badge}
                <p class="muted desc">{description}</p>
            </div>
            <a class="btn" href={route}>"Open challenge"</a>
        </article>
    }
}
