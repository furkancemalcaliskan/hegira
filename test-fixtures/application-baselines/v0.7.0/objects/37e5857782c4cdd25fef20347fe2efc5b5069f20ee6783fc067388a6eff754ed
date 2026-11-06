use icons::{ChevronDown, ChevronRight, Languages, LogOut, Menu, Moon, Settings, Sun, UserRound};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;

use crate::shared::i18n::{T, use_i18n};
use identity_leptos::{app::auth_state::AuthState, identity::auth::server_fns::logout};
use leptos_support::{
    feedback::toast::use_toast,
    rust_ui::ui::{
        avatar::{Avatar, AvatarFallback},
        button::{Button, ButtonSize, ButtonVariant},
        dropdown_menu::{DropdownMenuItem, DropdownMenuSeparator},
    },
};

#[component]
#[allow(unused_parens)] // Prevent adjacent braces from being parsed as template variables.
pub fn Topbar(page_title: Signal<T>, sidebar_open: RwSignal<bool>) -> impl IntoView {
    let navigate = use_navigate();
    let auth = use_context::<AuthState>().unwrap_or_default();
    let toast = use_toast();
    let i18n = use_i18n();
    let logging_out = RwSignal::new(false);
    let is_dark_theme = RwSignal::new(false);
    let account_menu_open = RwSignal::new(false);
    let logout_confirm_open = RwSignal::new(false);

    #[cfg(feature = "hydrate")]
    {
        if let Some(window) = web_sys::window()
            && let Some(document) = window.document()
            && let Some(root) = document.document_element()
        {
            is_dark_theme.set(root.class_list().contains("dark"));
        }
    }

    let username = Signal::derive({
        let auth = auth.clone();
        move || auth.username().unwrap_or_else(|| "User".to_string())
    });
    let initials = move || {
        username
            .get()
            .chars()
            .next()
            .unwrap_or('U')
            .to_uppercase()
            .to_string()
    };

    let on_logout = Callback::new(move |_| {
        if logging_out.get_untracked() {
            return;
        }

        let navigate = navigate.clone();
        let auth = auth.clone();
        let toast = toast.clone();
        logging_out.set(true);

        spawn_local(async move {
            if let Err(err) = logout().await {
                toast.error(i18n.t_untracked(T::LogoutFailed), err.to_string());
            }

            auth.clear();
            logging_out.set(false);
            navigate("/", Default::default());
        });
    });

    let toggle_theme = move |_| {
        #[cfg(feature = "hydrate")]
        {
            if let Some(window) = web_sys::window()
                && let Some(document) = window.document()
                && let Some(root) = document.document_element()
            {
                let class_list = root.class_list();
                let is_dark = class_list.contains("dark");
                if is_dark {
                    let _ = class_list.remove_1("dark");
                    is_dark_theme.set(false);
                    if let Ok(Some(storage)) = window.local_storage() {
                        let _ = storage.set_item("application-theme", "light");
                    }
                } else {
                    let _ = class_list.add_1("dark");
                    is_dark_theme.set(true);
                    if let Ok(Some(storage)) = window.local_storage() {
                        let _ = storage.set_item("application-theme", "dark");
                    }
                }
            }
        }
    };

    let toggle_language = move |_| {
        i18n.toggle_locale();
        account_menu_open.set(false);
    };

    view! {
        <>
        <header class="topbar">
            <div class="topbar-title">
                <Button
                    attr:r#type="button"
                    variant=ButtonVariant::Outline
                    size=ButtonSize::Icon
                    class="topbar-menu-button".to_string()
                    attr:aria-label=move || i18n.t(T::OpenNavigation)
                    on:click=move |_| sidebar_open.set(true)
                >
                    <Menu class="size-5" />
                </Button>

                <div class="min-w-0">
                    <nav aria-label="Breadcrumb">
                        <ol>
                            {move || (page_title.get() != T::Home).then(|| {
                                view! {
                                    <li>
                                        <A href="/dashboard">{move || i18n.t(T::Home)}</A>
                                    </li>
                                    <li aria-hidden="true">
                                        <ChevronRight class="size-3" />
                                    </li>
                                }
                            })}
                            <li>{move || i18n.t(page_title.get())}</li>
                        </ol>
                    </nav>
                </div>
            </div>

            <div class="topbar-actions">
                <Button
                    attr:r#type="button"
                    variant=ButtonVariant::Outline
                    size=ButtonSize::Icon
                    class="h-11 w-11".to_string()
                    attr:aria-label=move || i18n.t(T::ToggleTheme)
                    on:click=toggle_theme
                >
                    {(move || {
                        if is_dark_theme.get() {
                            view! { <Sun class="size-4" /> }.into_any()
                        } else {
                            view! { <Moon class="size-4" /> }.into_any()
                        }
                    })}
                </Button>

                <div class="relative">
                    <Button
                        attr:r#type="button"
                        variant=ButtonVariant::Ghost
                        class=move || {
                            if account_menu_open.get() {
                                "h-11 min-w-56 justify-start gap-2 rounded-md border bg-accent px-2.5 py-1 text-accent-foreground"
                            } else {
                                "h-11 min-w-56 justify-start gap-2 rounded-md border bg-background px-2.5 py-1 shadow-xs hover:bg-accent hover:text-accent-foreground"
                            }
                        }
                        attr:aria-expanded=move || account_menu_open.get().to_string()
                        attr:aria-haspopup="menu"
                        on:click=move |_| account_menu_open.update(|open| *open = !*open)
                    >
                        <Avatar>
                            <AvatarFallback>{initials}</AvatarFallback>
                        </Avatar>
                        <span class="grid min-w-0 flex-1 text-left leading-none">
                            <strong class="truncate text-sm font-medium leading-4">{move || username.get()}</strong>
                            <small class="truncate text-xs leading-4 text-muted-foreground">{move || i18n.t(T::Account)}</small>
                        </span>
                        <ChevronDown class="size-4 shrink-0 text-muted-foreground".to_string()/>
                    </Button>

                    <button
                        type="button"
                        class=move || {
                            if account_menu_open.get() {
                                "fixed inset-0 z-40 cursor-default bg-transparent"
                            } else {
                                "pointer-events-none fixed inset-0 z-40 cursor-default bg-transparent"
                            }
                        }
                        aria-label=move || i18n.t(T::CloseNavigation)
                        aria-hidden=move || (!account_menu_open.get()).to_string()
                        on:click=move |_| account_menu_open.set(false)
                    />
                    <ul
                        role="menu"
                        class=move || {
                            if account_menu_open.get() {
                                "account-menu is-open absolute right-0 top-full z-50 mt-2 w-56 rounded-md border bg-card p-1 text-card-foreground shadow-md"
                            } else {
                                "account-menu absolute right-0 top-full z-50 mt-2 w-56 rounded-md border bg-card p-1 text-card-foreground shadow-md"
                            }
                        }
                        aria-hidden=move || (!account_menu_open.get()).to_string()
                    >
                        <DropdownMenuItem on:click=move |_| account_menu_open.set(false)>
                            <A href="/profile" attr:class="flex w-full items-center gap-2">
                                <UserRound class="size-4".to_string()/>
                                {move || i18n.t(T::Profile)}
                            </A>
                        </DropdownMenuItem>
                        <DropdownMenuItem>
                            <Settings class="size-4".to_string()/>
                            {move || i18n.t(T::Settings)}
                        </DropdownMenuItem>
                        <DropdownMenuItem on:click=toggle_language>
                            <Languages class="size-4".to_string()/>
                            <span class="flex flex-1 items-center justify-between gap-3">
                                <span>{move || i18n.t(T::ToggleLanguage)}</span>
                                <span class="text-xs font-medium text-muted-foreground">
                                    {move || i18n.locale().label()}
                                </span>
                            </span>
                        </DropdownMenuItem>
                        <DropdownMenuSeparator/>
                        <DropdownMenuItem
                            class="text-destructive hover:bg-destructive/10 hover:text-destructive".to_string()
                            on:click=move |_| {
                                account_menu_open.set(false);
                                logout_confirm_open.set(true);
                            }
                        >
                            <LogOut class="size-4".to_string()/>
                            {(move || if logging_out.get() { i18n.t(T::LoggingOut) } else { i18n.t(T::Logout) })}
                        </DropdownMenuItem>
                    </ul>
                </div>

            </div>
        </header>

            <div
                class=move || {
                    if logout_confirm_open.get() {
                        "modal-overlay is-open fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
                    } else {
                        "modal-overlay pointer-events-none fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
                    }
                }
                aria-hidden=move || (!logout_confirm_open.get()).to_string()
            >
                    <div class=move || {
                        if logout_confirm_open.get() {
                            "modal-panel is-open w-full max-w-md rounded-lg border bg-background p-6 text-foreground shadow-lg"
                        } else {
                            "modal-panel w-full max-w-md rounded-lg border bg-background p-6 text-foreground shadow-lg"
                        }
                    }>
                        <div class="grid gap-2">
                            <h2 class="text-lg font-semibold">{move || i18n.t(T::ConfirmLogout)}</h2>
                            <p class="text-sm text-muted-foreground">{move || i18n.t(T::ConfirmLogoutDescription)}</p>
                        </div>
                        <div class="mt-6 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
                            <Button
                                variant=ButtonVariant::Outline
                                on:click=move |_| logout_confirm_open.set(false)
                            >
                                {move || i18n.t(T::Cancel)}
                            </Button>
                            <Button
                                variant=ButtonVariant::Destructive
                                on:click=move |_| {
                                    logout_confirm_open.set(false);
                                    on_logout.run(());
                                }
                            >
                                <LogOut class="size-4".to_string()/>
                                {(move || if logging_out.get() { i18n.t(T::LoggingOut) } else { i18n.t(T::Logout) })}
                            </Button>
                        </div>
                    </div>
                </div>
        </>
    }
}
