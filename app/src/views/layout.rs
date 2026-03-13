use leptos::prelude::*;

const STYLE: &str = include_str!("../style.css");

#[component]
pub fn PageShell(title: String, children: Children) -> impl IntoView {
    view! {
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <title>{title}</title>
                <style>{STYLE}</style>
            </head>
            <body>
                <main class="container">
                    {children()}
                </main>
            </body>
        </html>
    }
}
