use icons::Languages;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::{
    app::auth_state::AuthState,
    identity::auth::server_fns::{current_user, login, oauth_authorize, oauth_providers, register},
    shared::{
        i18n::{T, use_i18n},
        rust_ui::ui::{
            alert::{Alert, AlertDescription, AlertTitle},
            button::{Button, ButtonSize, ButtonVariant},
            card::{Card, CardContent},
            input::{Input, InputType},
            label::Label,
            spinner::Spinner,
        },
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoginGateStatus {
    Checking,
    Ready,
}

#[component]
pub fn LoginRoute() -> impl IntoView {
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(Option::<String>::None);
    let success = RwSignal::new(Option::<String>::None);
    let pending = RwSignal::new(false);
    let is_register = RwSignal::new(false);
    let navigate = use_navigate();
    let auth = use_context::<AuthState>().unwrap_or_default();
    let i18n = use_i18n();
    let login_status = RwSignal::new(LoginGateStatus::Checking);
    let oauth_options = RwSignal::new(Vec::<String>::new());

    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(providers) = oauth_providers().await {
                oauth_options.set(providers);
            }
        });
    });

    Effect::new({
        let navigate = navigate.clone();
        let auth = auth.clone();
        move |_| {
            let navigate = navigate.clone();
            let auth = auth.clone();
            spawn_local(async move {
                match current_user().await {
                    Ok(user) => {
                        auth.set_authenticated(Some(user.username), user.permissions);
                        navigate("/dashboard", Default::default());
                    }
                    Err(_) => {
                        auth.clear();
                        login_status.set(LoginGateStatus::Ready);
                    }
                }
            });
        }
    });

    let on_submit = Callback::new(move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();

        if pending.get_untracked() {
            return;
        }

        let username_value = username.get_untracked();
        let password_value = password.get_untracked();

        if username_value.trim().is_empty() || password_value.is_empty() {
            error.set(Some(i18n.t_untracked(T::CredentialsRequired).to_string()));
            return;
        }

        let navigate = navigate.clone();
        pending.set(true);
        error.set(None);
        success.set(None);

        if is_register.get_untracked() {
            spawn_local(async move {
                match register(username_value, password_value).await {
                    Ok(()) => {
                        success.set(Some(i18n.t_untracked(T::AccountCreated).to_string()));
                        is_register.set(false);
                        password.set(String::new());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                pending.set(false);
            });
        } else {
            let auth = auth.clone();
            spawn_local(async move {
                match login(username_value, password_value).await {
                    Ok(user) => {
                        auth.set_authenticated(Some(user.username), user.permissions);
                        navigate("/dashboard", Default::default());
                    }
                    Err(_) => {
                        error.set(Some(i18n.t_untracked(T::InvalidCredentials).to_string()));
                        pending.set(false);
                    }
                }
            });
        }
    });

    let switch_mode = Callback::new(move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        is_register.set(!is_register.get_untracked());
        error.set(None);
        success.set(None);
        password.set(String::new());
    });

    let toggle_language = Callback::new(move |_| {
        i18n.toggle_locale();
    });

    let start_oauth = Callback::new(move |provider: String| {
        error.set(None);
        spawn_local(async move {
            match oauth_authorize(provider, false).await {
                Ok(url) => redirect_to(&url),
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    });

    view! {
        {move || {
            if login_status.get() != LoginGateStatus::Ready {
                return view! { <main class="min-h-screen bg-background" /> }.into_any();
            }

            view! {
        <main class="route-fade grid min-h-screen grid-cols-1 bg-background text-foreground lg:grid-cols-2">
            <div class="absolute right-4 top-4 z-20">
                <Button
                    attr:r#type="button"
                    variant=ButtonVariant::Outline
                    size=ButtonSize::Sm
                    attr:aria-label=move || i18n.t(T::ToggleLanguage)
                    on:click=move |_| toggle_language.run(())
                >
                    <Languages class="size-4".to_string()/>
                    {move || i18n.locale().label()}
                </Button>
            </div>

            <section class="flex min-h-screen items-center justify-center px-6 py-10">
                <Card class="w-full max-w-md border-0 shadow-none sm:border sm:shadow-sm".to_string()>
                    <CardContent class="grid gap-6 px-6".to_string()>
                        <div class="grid gap-2 text-center">
                            <img
                                class="mx-auto size-10 object-contain"
                                src="/assets/branding/hegira-logo.png"
                                alt="Hegira"
                            />
                            <h1 class="text-2xl font-semibold tracking-tight">
                                {move || if is_register.get() { i18n.t(T::CreateAccount) } else { i18n.t(T::WelcomeBack) }}
                            </h1>
                            <p class="text-sm text-muted-foreground">
                                {move || {
                                    if is_register.get() {
                                        i18n.t(T::CreateWorkspaceAccount)
                                    } else {
                                        i18n.t(T::SignInWorkspace)
                                    }
                                }}
                            </p>
                        </div>

                        {move || {
                            error.get().map(|message| {
                                view! {
                                    <Alert class="border-destructive/30 bg-destructive/5 text-destructive".to_string()>
                                        <AlertTitle>{move || i18n.t(T::AuthenticationFailed)}</AlertTitle>
                                        <AlertDescription>{message}</AlertDescription>
                                    </Alert>
                                }
                            })
                        }}

                        {move || {
                            success.get().map(|message| {
                                view! {
                                    <Alert class="border-success/30 bg-success/5 text-success".to_string()>
                                        <AlertTitle>{move || i18n.t(T::AccountReady)}</AlertTitle>
                                        <AlertDescription>{message}</AlertDescription>
                                    </Alert>
                                }
                            })
                        }}

                        <form class="grid gap-4" on:submit=move |ev| on_submit.run(ev)>
                            <div class="grid gap-2">
                                <Label html_for="login-username">{move || i18n.t(T::Username)}</Label>
                                <Input
                                    id="login-username"
                                    r#type=InputType::Text
                                    placeholder="alice"
                                    autocomplete="username"
                                    bind_value=username
                                    required=true
                                />
                            </div>

                            <div class="grid gap-2">
                                <Label html_for="login-password">{move || i18n.t(T::Password)}</Label>
                                <Input
                                    id="login-password"
                                    r#type=InputType::Password
                                    placeholder="••••••••"
                                    autocomplete="current-password"
                                    bind_value=password
                                    required=true
                                />
                            </div>

                            <Button class="w-full".to_string()>
                                {move || pending.get().then(|| view! { <Spinner class="size-4".to_string()/> })}
                                {move || {
                                    if pending.get() {
                                        if is_register.get() { i18n.t(T::Creating) } else { i18n.t(T::SigningIn) }
                                    } else if is_register.get() {
                                        i18n.t(T::CreateAccount)
                                    } else {
                                        i18n.t(T::SignIn)
                                    }
                                }}
                            </Button>
                        </form>

                        {move || (!oauth_options.get().is_empty()).then(|| view! {
                            <div class="grid gap-3">
                                <div class="flex items-center gap-3">
                                    <div class="h-px flex-1 bg-border"></div>
                                    <span class="text-xs text-muted-foreground">{move || i18n.t(T::Or)}</span>
                                    <div class="h-px flex-1 bg-border"></div>
                                </div>
                                <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
                                    {oauth_options.get().into_iter().map(|provider| {
                                        let label = match provider.as_str() {
                                            "google" => T::ContinueWithGoogle,
                                            "github" => T::ContinueWithGithub,
                                            _ => T::OAuthAuthentication,
                                        };
                                        let value = provider.clone();
                                        view! {
                                            <Button
                                                attr:r#type="button"
                                                variant=ButtonVariant::Outline
                                                class="w-full".to_string()
                                                on:click=move |_| start_oauth.run(value.clone())
                                            >
                                                {move || i18n.t(label)}
                                            </Button>
                                        }
                                    }).collect_view()}
                                </div>
                            </div>
                        })}

                        <Button
                            variant=ButtonVariant::Ghost
                            class="w-full".to_string()
                            on:click=move |ev| switch_mode.run(ev)
                        >
                            {move || {
                                if is_register.get() {
                                    i18n.t(T::AlreadyHaveAccount)
                                } else {
                                    i18n.t(T::NeedAccount)
                                }
                            }}
                        </Button>
                    </CardContent>
                </Card>
            </section>

            <section class="hidden min-h-screen overflow-hidden border-l bg-muted lg:block">
                <div class="relative flex h-full flex-col justify-between p-10">
                    <div class="absolute inset-0 bg-card"></div>
                    <div class="absolute inset-0 bg-[linear-gradient(to_right,var(--border)_1px,transparent_1px),linear-gradient(to_bottom,var(--border)_1px,transparent_1px)] bg-[size:44px_44px] opacity-40"></div>
                    <div class="relative z-10 flex items-center gap-3 text-white">
                        <img
                            class="size-10 object-contain"
                            src="/assets/branding/hegira-logo.png"
                            alt=""
                            aria-hidden="true"
                        />
                        <div class="grid">
                            <strong>"Hegira"</strong>
                            <span class="text-sm text-white/65">{move || i18n.t(T::LoginHeroKicker)}</span>
                        </div>
                    </div>
                    <div class="relative z-10 max-w-xl text-white">
                        <p class="text-3xl font-semibold leading-tight">
                            {move || i18n.t(T::LoginHeroTitle)}
                        </p>
                        <p class="mt-4 text-sm leading-6 text-white/70">
                            {move || i18n.t(T::LoginHeroDescription)}
                        </p>
                    </div>
                </div>
            </section>
        </main>
            }.into_any()
        }}
    }
}

fn redirect_to(url: &str) {
    #[cfg(feature = "hydrate")]
    if let Some(window) = web_sys::window() {
        let _ = window.location().set_href(url);
    }
    #[cfg(not(feature = "hydrate"))]
    let _ = url;
}
