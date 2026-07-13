use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::{
    application_contracts::identity::auth::CurrentUserDto,
    web::{
        app::{
            layout::WorkspaceRouteLayout,
            page::{PageHeaderKey, PageSection},
        },
        identity::auth::server_fns::{
            current_user, oauth_authorize, oauth_connections, oauth_providers,
            unlink_oauth_connection,
        },
        shared::{
            i18n::{T, use_i18n},
            rust_ui::ui::{
                alert::{Alert, AlertDescription},
                avatar::{Avatar, AvatarFallback},
                badge::{Badge, BadgeVariant},
                button::{Button, ButtonVariant},
                dialog::{DialogBody, DialogDescription, DialogFooter, DialogHeader, DialogTitle},
                skeleton::Skeleton,
            },
        },
    },
};

#[component]
pub fn ProfileRoute() -> impl IntoView {
    let i18n = use_i18n();
    let loading = RwSignal::new(true);
    let error = RwSignal::new(Option::<String>::None);
    let profile = RwSignal::new(Option::<CurrentUserDto>::None);
    let connections = RwSignal::new(Vec::<
        application_contracts::identity::auth::OAuthConnectionDto,
    >::new());
    let providers = RwSignal::new(Vec::<String>::new());
    let confirm_disconnect = RwSignal::new(None::<String>);
    let disconnecting = RwSignal::new(false);

    Effect::new(move |_| {
        loading.set(true);
        error.set(None);

        spawn_local(async move {
            match current_user().await {
                Ok(user) => profile.set(Some(user)),
                Err(err) => error.set(Some(err.to_string())),
            }
            if let Ok(items) = oauth_connections().await {
                connections.set(items);
            }
            if let Ok(items) = oauth_providers().await {
                providers.set(items);
            }
            loading.set(false);
        });
    });

    let connect = Callback::new(move |provider: String| {
        spawn_local(async move {
            match oauth_authorize(provider, true).await {
                Ok(url) => redirect_to(&url),
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    });

    let disconnect = Callback::new(move |provider: String| {
        if disconnecting.get_untracked() {
            return;
        }
        disconnecting.set(true);
        spawn_local(async move {
            match unlink_oauth_connection(provider.clone()).await {
                Ok(()) => {
                    connections.update(|items| items.retain(|item| item.provider != provider))
                }
                Err(err) => error.set(Some(err.to_string())),
            }
            disconnecting.set(false);
            confirm_disconnect.set(None);
        });
    });

    view! {
        <WorkspaceRouteLayout title=T::Profile>
            <div class="page-stack">
                <PageHeaderKey title=T::Profile description=T::ProfileDescription/>
                <PageSection>
                    {move || {
                        if loading.get() {
                            view! {
                                <div class="profile-summary">
                                    <Skeleton class="size-14 rounded-full".to_string()/>
                                    <div class="grid gap-2">
                                        <Skeleton class="h-6 w-44".to_string()/>
                                        <Skeleton class="h-4 w-64".to_string()/>
                                    </div>
                                </div>
                            }.into_any()
                        } else if let Some(message) = error.get() {
                            view! {
                                <Alert class="border-destructive/30 bg-destructive/5 text-destructive".to_string()>
                                    <AlertDescription>{message}</AlertDescription>
                                </Alert>
                            }.into_any()
                        } else if let Some(user) = profile.get() {
                            let initial = user.username.chars().next().unwrap_or('U').to_uppercase().to_string();
                            let permission_count = user.permissions.len();
                            view! {
                                <div class="grid gap-5">
                                    <div class="profile-summary">
                                        <Avatar class="size-14".to_string()>
                                            <AvatarFallback>{initial}</AvatarFallback>
                                        </Avatar>
                                        <div>
                                            <h2>{user.username.clone()}</h2>
                                            <p>{format!("{permission_count} {}", i18n.t(T::RolePermissions).to_lowercase())}</p>
                                        </div>
                                    </div>
                                    <div class="flex flex-wrap gap-2">
                                        {user.permissions.into_iter().map(|permission| {
                                            view! {
                                                <Badge variant=BadgeVariant::Secondary>{permission}</Badge>
                                            }
                                        }).collect_view()}
                                    </div>
                                </div>
                            }.into_any()
                        } else {
                            view! { <div/> }.into_any()
                        }
                    }}
                </PageSection>
                {move || (!providers.get().is_empty()).then(|| {
                    view! {
                        <PageSection>
                            <div class="grid gap-4">
                                <div>
                                    <h2 class="font-semibold">{move || i18n.t(T::ConnectedAccounts)}</h2>
                                    <p class="text-sm text-muted-foreground">{move || i18n.t(T::ConnectedAccountsDescription)}</p>
                                </div>
                                <div class="grid gap-2">
                                    {providers.get().into_iter().map(|provider| {
                                        let connection = connections.get().into_iter().find(|item| item.provider == provider);
                                        let title = match provider.as_str() { "google" => "Google", "github" => "GitHub", _ => "OAuth" };
                                        let action_provider = provider.clone();
                                        if let Some(connection) = connection {
                                            view! {
                                                <div class="flex items-center justify-between gap-4 rounded-md border p-3">
                                                    <div class="min-w-0">
                                                        <p class="font-medium">{title}</p>
                                                        <p class="truncate text-sm text-muted-foreground">{connection.email}</p>
                                                    </div>
                                                    <Button variant=ButtonVariant::Outline on:click=move |_| confirm_disconnect.set(Some(action_provider.clone()))>{move || i18n.t(T::Disconnect)}</Button>
                                                </div>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <div class="flex items-center justify-between gap-4 rounded-md border p-3">
                                                    <p class="font-medium">{title}</p>
                                                    <Button variant=ButtonVariant::Outline on:click=move |_| connect.run(action_provider.clone())>{move || i18n.t(T::Connect)}</Button>
                                                </div>
                                            }.into_any()
                                        }
                                    }).collect_view()}
                                </div>
                            </div>
                        </PageSection>
                    }
                })}
            </div>
            <div
                class=move || if confirm_disconnect.get().is_some() {
                    "modal-overlay is-open fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
                } else {
                    "modal-overlay pointer-events-none fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
                }
                aria-hidden=move || confirm_disconnect.get().is_none().to_string()
            >
                <div class=move || if confirm_disconnect.get().is_some() {
                    "modal-panel is-open w-full max-w-md rounded-lg border bg-background p-6 text-foreground shadow-lg"
                } else {
                    "modal-panel w-full max-w-md rounded-lg border bg-background p-6 text-foreground shadow-lg"
                }>
                    <DialogBody>
                        <DialogHeader>
                            <DialogTitle>{move || i18n.t(T::ConfirmDisconnect)}</DialogTitle>
                            <DialogDescription>{move || i18n.t(T::ConfirmDisconnectDescription)}</DialogDescription>
                        </DialogHeader>
                        <DialogFooter>
                            <Button
                                variant=ButtonVariant::Outline
                                attr:disabled=move || disconnecting.get()
                                on:click=move |_| confirm_disconnect.set(None)
                            >
                                {move || i18n.t(T::Cancel)}
                            </Button>
                            <Button
                                variant=ButtonVariant::Destructive
                                attr:disabled=move || disconnecting.get()
                                on:click=move |_| {
                                    if let Some(provider) = confirm_disconnect.get_untracked() {
                                        disconnect.run(provider);
                                    }
                                }
                            >
                                {move || i18n.t(T::Disconnect)}
                            </Button>
                        </DialogFooter>
                    </DialogBody>
                </div>
            </div>
        </WorkspaceRouteLayout>
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
