use leptos::prelude::*;

use super::super::layout::PageShell;

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
