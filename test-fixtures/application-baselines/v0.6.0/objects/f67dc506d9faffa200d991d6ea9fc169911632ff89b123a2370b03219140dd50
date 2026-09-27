use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <script>
                    {r#"
                    (function () {
                        if (!window.ScrollLock) {
                            let locks = 0;
                            let previousOverflow = '';
                            window.ScrollLock = {
                                lock: function () {
                                    locks += 1;
                                    if (locks === 1) {
                                        previousOverflow = document.documentElement.style.overflow || '';
                                        document.documentElement.style.overflow = 'hidden';
                                    }
                                },
                                unlock: function (delay) {
                                    window.setTimeout(function () {
                                        locks = Math.max(0, locks - 1);
                                        if (locks === 0) {
                                            document.documentElement.style.overflow = previousOverflow;
                                        }
                                    }, delay || 0);
                                }
                            };
                        }

                        var theme = localStorage.getItem('application-theme') || 'system';
                        var prefersDark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
                        if (theme === 'dark' || (theme === 'system' && prefersDark)) {
                            document.documentElement.classList.add('dark');
                        }
                    })();
                    "#}
                </script>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/app.css"/>
        <Title text="Application"/>
        <BootLoader/>
        <BootLoaderDismiss/>
        <crate::app::providers::AppProviders>
            <crate::routes::WebRoutes/>
        </crate::app::providers::AppProviders>
    }
}

#[component]
fn BootLoader() -> impl IntoView {
    view! {
        <div id="wasm-boot-loader" class="wasm-boot-loader" aria-live="polite" aria-label="Loading application">
            <img
                class="brand-logo"
                src="/assets/branding/hegira-logo.png"
                alt=""
                aria-hidden="true"
            />
            <div class="wasm-boot-spinner" aria-hidden="true"></div>
        </div>
    }
}

#[component]
fn BootLoaderDismiss() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    Effect::new(|_| {
        if let Some(document) = web_sys::window().and_then(|window| window.document())
            && let Some(loader) = document.get_element_by_id("wasm-boot-loader")
        {
            let _ = loader.class_list().add_1("is-hidden");
        }
    });
}
