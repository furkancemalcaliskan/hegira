use icons::Save;
use leptos::prelude::*;

use crate::{
    identity::roles::model::roles_page_state::RolesPageState,
    shared::{
        crud::dialog::CrudDialog,
        i18n::{T, use_i18n},
        rust_ui::ui::{
            alert::{Alert, AlertDescription},
            button::{Button, ButtonVariant},
            dialog::DialogFooter,
            input::{Input, InputType},
            label::Label,
            spinner::Spinner,
        },
    },
};

#[component]
pub fn RoleFormPanel(
    state: RolesPageState,
    on_cancel: Callback<()>,
    on_submit: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    let title = Signal::derive(move || {
        if state.editing_role.get().is_some() {
            i18n.t_untracked(T::EditRole).to_string()
        } else {
            i18n.t_untracked(T::CreateRole).to_string()
        }
    });
    let description = Signal::derive(move || {
        if state.editing_role.get().is_some() {
            i18n.t_untracked(T::EditRoleDescription).to_string()
        } else {
            i18n.t_untracked(T::CreateRoleDescription).to_string()
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
                    <Label html_for="role-name">{move || i18n.t(T::RoleName)}</Label>
                    <Input
                        id="role-name"
                        r#type=InputType::Text
                        placeholder="manager"
                        bind_value=state.role_name
                        required=true
                        on:input=move |_| state.form_error.set(None)
                    />
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
                        {move || if state.mutation_status.get().is_pending() {
                            view! { <Spinner class="size-4".to_string()/> }.into_any()
                        } else {
                            view! { <Save class="size-4".to_string()/> }.into_any()
                        }}
                        {move || if state.mutation_status.get().is_pending() { i18n.t(T::Saving) } else { i18n.t(T::SaveRole) }}
                    </Button>
                </DialogFooter>
            </form>
        </CrudDialog>
    }
}
