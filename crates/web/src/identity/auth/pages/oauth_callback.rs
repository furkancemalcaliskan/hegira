use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};

use crate::{
    app::auth_state::AuthState,
    identity::auth::server_fns::{
        complete_oauth_signup, current_user, oauth_callback, verify_totp_login,
    },
    shared::{
        i18n::{T, use_i18n},
        rust_ui::ui::{
            alert::{Alert, AlertDescription},
            button::Button,
            card::{Card, CardContent, CardDescription, CardHeader, CardTitle},
            input::{Input, InputType},
            label::Label,
            spinner::Spinner,
        },
    },
};

#[component]
pub fn OAuthCallbackRoute() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let navigate = use_navigate();
    let auth = use_context::<AuthState>().unwrap_or_default();
    let loading = RwSignal::new(true);
    let pending = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let signup_token = RwSignal::new(None::<String>);
    let username = RwSignal::new(String::new());
    let totp_token = RwSignal::new(None::<String>);
    let totp_code = RwSignal::new(String::new());
    let i18n = use_i18n();

    Effect::new({
        let navigate = navigate.clone();
        let auth = auth.clone();
        move |_| {
            let provider = params.read().get("provider").unwrap_or_default();
            let provider_error = query.read().get("error").unwrap_or_default();
            let code = query.read().get("code").unwrap_or_default();
            let state = query.read().get("state").unwrap_or_default();
            if !provider_error.is_empty() {
                error.set(Some(i18n.t_untracked(T::OAuthCancelled).to_string()));
                loading.set(false);
                return;
            }
            if provider.is_empty() || code.is_empty() || state.is_empty() {
                error.set(Some(i18n.t_untracked(T::InvalidOAuthCallback).to_string()));
                loading.set(false);
                return;
            }

            let navigate = navigate.clone();
            let auth = auth.clone();
            spawn_local(async move {
                match oauth_callback(provider, code, state).await {
                    Ok(result) if result.linked => navigate("/profile", Default::default()),
                    Ok(result) => {
                        if let Some(login) = result.login {
                            if login.totp_required {
                                totp_token.set(login.totp_token);
                            } else {
                                match establish_session(auth).await {
                                    Ok(()) => navigate("/content", Default::default()),
                                    Err(message) => error.set(Some(message)),
                                }
                            }
                        } else if let Some(token) = result.signup_token {
                            username.set(result.suggested_username.unwrap_or_default());
                            signup_token.set(Some(token));
                        }
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                loading.set(false);
            });
        }
    });

    let submit_totp = Callback::new({
        let navigate = navigate.clone();
        let auth = auth.clone();
        move |event: leptos::ev::SubmitEvent| {
            event.prevent_default();
            let Some(token) = totp_token.get_untracked() else {
                return;
            };
            let code = totp_code.get_untracked();
            if code.trim().is_empty() {
                error.set(Some(i18n.t_untracked(T::TotpRequired).to_string()));
                return;
            }
            pending.set(true);
            error.set(None);
            let navigate = navigate.clone();
            let auth = auth.clone();
            spawn_local(async move {
                match verify_totp_login(token, code).await {
                    Ok(user) => {
                        auth.set_authenticated(Some(user.username), user.permissions);
                        navigate("/content", Default::default());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                pending.set(false);
            });
        }
    });

    let submit = Callback::new({
        let navigate = navigate.clone();
        let auth = auth.clone();
        move |event: leptos::ev::SubmitEvent| {
            event.prevent_default();
            let Some(token) = signup_token.get_untracked() else {
                return;
            };
            let selected_username = username.get_untracked();
            if selected_username.trim().is_empty() {
                error.set(Some(i18n.t_untracked(T::UsernameRequired).to_string()));
                return;
            }
            pending.set(true);
            error.set(None);
            let navigate = navigate.clone();
            let auth = auth.clone();
            spawn_local(async move {
                match complete_oauth_signup(token, selected_username).await {
                    Ok(user) => {
                        auth.set_authenticated(Some(user.username), user.permissions);
                        navigate("/content", Default::default());
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                pending.set(false);
            });
        }
    });

    view! {
        <main class="route-fade grid min-h-screen place-items-center bg-background px-6 text-foreground">
            <Card class="w-full max-w-md".to_string()>
                <CardHeader>
                    <CardTitle>{move || i18n.t(T::OAuthAuthentication)}</CardTitle>
                    <CardDescription>{move || i18n.t(T::OAuthCompleting)}</CardDescription>
                </CardHeader>
                <CardContent class="grid gap-4".to_string()>
                    {move || error.get().map(|message| view! {
                        <Alert class="border-destructive/30 bg-destructive/5 text-destructive".to_string()>
                            <AlertDescription>{message}</AlertDescription>
                        </Alert>
                    })}
                    {move || if loading.get() {
                        view! { <div class="flex justify-center py-8"><Spinner class="size-6".to_string()/></div> }.into_any()
                    } else if totp_token.get().is_some() {
                        view! {
                            <form class="grid gap-4" on:submit=move |event| submit_totp.run(event)>
                                <div class="grid gap-2">
                                    <Label html_for="oauth-totp">{move || i18n.t(T::TotpCode)}</Label>
                                    <Input
                                        id="oauth-totp"
                                        r#type=InputType::Tel
                                        autocomplete="one-time-code"
                                        bind_value=totp_code
                                        required=true
                                    />
                                </div>
                                <Button class="w-full".to_string() attr:disabled=move || pending.get()>
                                    {move || pending.get().then(|| view! { <Spinner class="size-4".to_string()/> })}
                                    {move || i18n.t(T::Verify)}
                                </Button>
                            </form>
                        }.into_any()
                    } else if signup_token.get().is_some() {
                        view! {
                            <form class="grid gap-4" on:submit=move |event| submit.run(event)>
                                <div class="grid gap-2">
                                    <Label html_for="oauth-username">{move || i18n.t(T::Username)}</Label>
                                    <Input id="oauth-username" r#type=InputType::Text bind_value=username required=true/>
                                </div>
                                <Button class="w-full".to_string() attr:disabled=move || pending.get()>
                                    {move || pending.get().then(|| view! { <Spinner class="size-4".to_string()/> })}
                                    {move || i18n.t(T::CompleteAccount)}
                                </Button>
                            </form>
                        }.into_any()
                    } else {
                        view! { <Button href="/login" class="w-full".to_string()>{move || i18n.t(T::BackToLogin)}</Button> }.into_any()
                    }}
                </CardContent>
            </Card>
        </main>
    }
}

async fn establish_session(auth: AuthState) -> Result<(), String> {
    let user = match current_user().await {
        Ok(user) => user,
        Err(err) => return Err(err.to_string()),
    };
    auth.set_authenticated(Some(user.username), user.permissions);
    Ok(())
}
