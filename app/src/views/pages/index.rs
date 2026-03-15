use leptos::prelude::*;

use super::super::{
    CategorySection,
    components::ChallengeCard,
    layout::PageShell,
    scripts::index_timer_script,
};

// ---------------------------------------------------------------------------
// Challenge list (index page)
// ---------------------------------------------------------------------------

#[component]
pub fn IndexPage(
    ctf_title: String,
    user_label: String,
    sections: Vec<CategorySection>,
) -> impl IntoView {
    let timer_script = index_timer_script();
    view! {
        <PageShell title=ctf_title.clone()>
            <header class="topbar">
                <div>
                    <h1>{ctf_title}</h1>
                    <p class="muted">"Logged in as " {user_label}</p>
                </div>
                <div class="nav-buttons">
                    <a class="btn secondary" href="/scoreboard">"Scoreboard"</a>
                    <form method="post" action="/logout" style="display:inline-block;">
                        <button class="btn secondary" type="submit">"Logout"</button>
                    </form>
                </div>
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
            <script>{timer_script}</script>
        </PageShell>
    }
}
