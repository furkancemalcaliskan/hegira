use icons::{CircleUser, House, PanelLeftClose, PanelLeftOpen, Search, Users};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_location;

use crate::{
    app::navigation::{NavIcon, WORKSPACE_NAV, workspace_nav_items},
    shared::{
        authorization,
        i18n::{T, use_i18n},
        rust_ui::ui::{
            button::{Button, ButtonSize, ButtonVariant},
            input::{Input, InputType},
        },
    },
};

#[component]
pub fn Sidebar(sidebar_open: RwSignal<bool>, sidebar_collapsed: RwSignal<bool>) -> impl IntoView {
    let location = use_location();
    let i18n = use_i18n();
    let sidebar_hovered = RwSignal::new(false);
    let is_collapsed = move || sidebar_collapsed.get() && !sidebar_hovered.get();
    let aside_class = move || {
        let mut class = "app-sidebar".to_string();
        if sidebar_open.get() {
            class.push_str(" is-open");
        }
        if sidebar_collapsed.get() && sidebar_hovered.get() {
            class.push_str(" is-hover-expanded");
        }
        class
    };
    let shell_class = move || {
        if is_collapsed() {
            "brand-shell is-collapsed"
        } else {
            "brand-shell"
        }
    };

    view! {
        <Show when=move || sidebar_open.get()>
            <button
                type="button"
                class="sidebar-scrim"
                aria-label=move || i18n.t(T::CloseNavigation)
                on:click=move |_| sidebar_open.set(false)
            />
        </Show>

        <aside
            class=aside_class
            aria-label="Main navigation"
            on:mouseenter=move |_| {
                if sidebar_collapsed.get_untracked() {
                    sidebar_hovered.set(true);
                }
            }
            on:mouseleave=move |_| {
                if sidebar_collapsed.get_untracked() {
                    sidebar_hovered.set(false);
                }
            }
        >
            <div class=shell_class>
                <A href="/dashboard" attr:class="brand-mark" attr:aria-label="hegira home">
                    <img
                        class="brand-logo"
                        src="/assets/branding/hegira-logo.png"
                        alt=""
                        aria-hidden="true"
                    />
                    <span class="brand-copy">
                        <strong>"Hegira"</strong>
                        <small>"Leptos Console"</small>
                    </span>
                </A>

                <Show when=move || !is_collapsed()>
                    <Button
                        attr:r#type="button"
                        variant=ButtonVariant::Outline
                        size=ButtonSize::Icon
                        class="sidebar-collapse".to_string()
                        attr:aria-label=move || i18n.t(T::ToggleCompactSidebar)
                        on:click=move |_| {
                            if sidebar_collapsed.get_untracked() {
                                sidebar_collapsed.set(false);
                                sidebar_hovered.set(false);
                            } else {
                                sidebar_collapsed.set(true);
                            }
                        }
                    >
                        <Show
                            when=move || sidebar_collapsed.get()
                            fallback=|| view! { <PanelLeftClose class="size-4" /> }
                        >
                            <PanelLeftOpen class="size-4" />
                        </Show>
                    </Button>
                </Show>
            </div>

            <div class="sidebar-search">
                <Search class="pointer-events-none absolute left-3 top-1/2 z-10 size-4 -translate-y-1/2 text-muted-foreground".to_string()/>
                <Input
                    class="pl-9".to_string()
                    r#type=InputType::Search
                    placeholder=i18n.t_untracked(T::Search).to_string()
                />
            </div>

            <nav class="sidebar-nav">
                {WORKSPACE_NAV
                    .iter()
                    .map(|section| {
                        view! {
                            <section class="sidebar-section">
                                <h2>{move || i18n.t(section.label)}</h2>
                                <div class="sidebar-items">
                                    {workspace_nav_items(section)
                                        .filter(|item| authorization::can_access_untracked(item.permission))
                                        .map(|item| {
                                            let is_active = move || location.pathname.get() == item.href;
                                            let item_class = move || {
                                                if is_active() {
                                                    "sidebar-link is-active"
                                                } else {
                                                    "sidebar-link"
                                                }
                                            };
                                            view! {
                                                <A
                                                    href=item.href
                                                    attr:class=item_class
                                                    attr:aria-current=move || is_active().then_some("page")
                                                    attr:title=move || i18n.t(item.label)
                                                    on:click=move |_| sidebar_open.set(false)
                                                >
                                                    <NavItemIcon icon=item.icon/>
                                                    <span>{move || i18n.t(item.label)}</span>
                                                </A>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            </section>
                        }
                    })
                    .collect_view()}
            </nav>

            <footer class="sidebar-version">
                <strong>"Hegira"</strong>
                <small>
                    {move || format!("v{} · {}", env!("CARGO_PKG_VERSION"), i18n.t(T::AllRightsReserved))}
                </small>
            </footer>
        </aside>
    }
}

#[component]
fn NavItemIcon(icon: NavIcon) -> impl IntoView {
    match icon {
        NavIcon::Home => view! { <House class="size-4" /> }.into_any(),
        NavIcon::Roles => view! { <Users class="size-4" /> }.into_any(),
        NavIcon::Users => view! { <Users class="size-4" /> }.into_any(),
        NavIcon::Profile => view! { <CircleUser class="size-4" /> }.into_any(),
    }
}
