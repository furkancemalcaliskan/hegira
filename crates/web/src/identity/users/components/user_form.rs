use icons::Save;
use leptos::prelude::*;

use crate::{
    identity::users::model::users_page_state::UsersPageState,
    shared::{
        crud::dialog::CrudDialog,
        i18n::{T, use_i18n},
        rust_ui::ui::{
            alert::{Alert, AlertDescription},
            button::{Button, ButtonVariant},
            checkbox::Checkbox,
            dialog::DialogFooter,
            input::{Input, InputType},
            label::Label,
            spinner::Spinner,
        },
    },
};

#[component]
pub fn UserFormPanel(
    state: UsersPageState,
    on_cancel: Callback<()>,
    on_submit: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    let is_editing_now = move || state.editing_username.get_untracked().is_some();
    let title = Signal::derive(move || {
        if state.editing_username.get().is_some() {
            i18n.t_untracked(T::EditUser).to_string()
        } else {
            i18n.t_untracked(T::CreateUser).to_string()
        }
    });
    let description = Signal::derive(move || {
        if state.editing_username.get().is_some() {
            i18n.t_untracked(T::EditUserDescription).to_string()
        } else {
            i18n.t_untracked(T::CreateUserDescription).to_string()
        }
    });

    view! {
        <CrudDialog open=state.form_open title=title description=description on_close=on_cancel>
            {move || {
                state.form_error.get().map(|message| {
                    view! {
                        <Alert class="border-destructive/30 bg-destructive/5 text-destructive".to_string()>
                            <AlertDescription>{message}</AlertDescription>
                        </Alert>
                    }
                })
            }}

            <form
                class="grid gap-4"
                on:submit=move |ev| {
                    ev.prevent_default();
                    on_submit.run(());
                }
            >
                <div class="grid gap-2">
                    <Label html_for="user-username">{move || i18n.t(T::Username)}</Label>
                    <Input
                        id="user-username"
                        r#type=InputType::Text
                        placeholder="alice"
                        bind_value=state.username
                        readonly=is_editing_now()
                        required=true
                        on:input=move |_| state.form_error.set(None)
                    />
                </div>

                <div class="grid gap-2">
                    <Label html_for="user-password">{move || i18n.t(T::Password)}</Label>
                    <Input
                        id="user-password"
                        r#type=InputType::Password
                        placeholder=if is_editing_now() { i18n.t_untracked(T::PasswordKeepCurrent).to_string() } else { i18n.t_untracked(T::TemporaryPassword).to_string() }
                        bind_value=state.password
                        required=!is_editing_now()
                        on:input=move |_| state.form_error.set(None)
                    />
                </div>

                <label class="flex items-start gap-3 rounded-md border p-3">
                    <Checkbox
                        checked=Signal::derive(move || state.is_verified.get())
                        on_checked_change=Callback::new(move |checked| {
                            state.is_verified.set(checked);
                            state.form_error.set(None);
                        })
                        aria_label=i18n.t_untracked(T::Verified).to_string()
                    />
                    <span class="grid gap-1">
                        <strong>{move || i18n.t(T::Verified)}</strong>
                        <small class="text-muted-foreground">{move || i18n.t(T::VerifiedDescription)}</small>
                    </span>
                </label>

                <div class="grid gap-2">
                    <div class="grid gap-1">
                        <Label>{move || i18n.t(T::Roles)}</Label>
                        <p class="text-sm text-muted-foreground">
                            {move || i18n.t(T::UserRolesDescription)}
                        </p>
                    </div>
                    <div class="grid max-h-48 gap-2 overflow-auto rounded-md border p-2">
                        {move || {
                            state.roles.get().into_iter().map(|role| {
                                let role_name = role.name.clone();
                                let role_for_checked = role.name.clone();
                                let role_for_change = role.name.clone();
                                view! {
                                    <label class="flex items-start gap-3 rounded-md p-2 hover:bg-muted/60">
                                        <Checkbox
                                            checked=Signal::derive(move || {
                                                state.selected_roles.get().iter().any(|item| item == &role_for_checked)
                                            })
                                            on_checked_change=Callback::new(move |checked| {
                                                state.toggle_role(role_for_change.clone(), checked);
                                                state.form_error.set(None);
                                            })
                                            aria_label=role_name.clone()
                                        />
                                        <span class="grid gap-1">
                                            <strong>{role_name}</strong>
                                            <small class="text-muted-foreground">
                                                {format!("{} {}", role.permissions.len(), i18n.t_untracked(T::RolePermissions).to_lowercase())}
                                            </small>
                                        </span>
                                    </label>
                                }
                            }).collect_view()
                        }}
                    </div>
                </div>

                <DialogFooter>
                    <Button
                        variant=ButtonVariant::Outline
                        on:click=move |ev| {
                            ev.prevent_default();
                            on_cancel.run(());
                        }
                    >
                        {move || i18n.t(T::Cancel)}
                    </Button>
                    <Button>
                        {move || if state.save_status.get().is_pending() {
                            view! { <Spinner class="size-4".to_string()/> }.into_any()
                        } else {
                            view! { <Save class="size-4".to_string()/> }.into_any()
                        }}
                        {move || if state.save_status.get().is_pending() { i18n.t(T::Saving) } else { i18n.t(T::SaveUser) }}
                    </Button>
                </DialogFooter>
            </form>
        </CrudDialog>
    }
}
