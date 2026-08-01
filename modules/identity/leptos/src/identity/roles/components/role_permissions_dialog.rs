use icons::Save;
use leptos::prelude::*;

use crate::{
    identity::roles::model::roles_page_state::RolesPageState,
    shared::{
        crud::dialog::CrudDialog,
        i18n::{T, use_i18n},
        rust_ui::ui::{
            button::{Button, ButtonVariant},
            checkbox::Checkbox,
            dialog::DialogFooter,
            spinner::Spinner,
        },
    },
};

#[component]
pub fn RolePermissionsDialog(
    state: RolesPageState,
    on_cancel: Callback<()>,
    on_submit: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    let title = Signal::derive(move || {
        let role = state.permissions_role.get().unwrap_or_default();
        format!("{}: {role}", i18n.t_untracked(T::RolePermissions))
    });
    let description =
        Signal::derive(move || i18n.t_untracked(T::RolePermissionsDescription).to_string());

    view! {
        <CrudDialog open=state.permissions_open title=title description=description on_close=on_cancel>
            <div class="grid max-h-[50vh] gap-2 overflow-auto pr-1">
                {move || {
                    state.permissions.get().into_iter().map(|permission| {
                        let name = permission.name.clone();
                        let name_for_checked = name.clone();
                        let name_for_change = name.clone();
                        view! {
                            <label class="flex items-start gap-3 rounded-md border p-3">
                                <Checkbox
                                    checked=Signal::derive(move || {
                                        state.selected_permissions.get().iter().any(|item| item == &name_for_checked)
                                    })
                                    on_checked_change=Callback::new(move |checked| {
                                        state.toggle_permission(name_for_change.clone(), checked);
                                    })
                                    aria_label=permission.display_name.clone()
                                />
                                <span class="grid gap-1">
                                    <strong>{permission.display_name}</strong>
                                    <small class="text-muted-foreground">{name}</small>
                                </span>
                            </label>
                        }
                    }).collect_view()
                }}
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
                <Button on:click=move |_| on_submit.run(())>
                    {move || if state.mutation_status.get().is_pending() {
                        view! { <Spinner class="size-4".to_string()/> }.into_any()
                    } else {
                        view! { <Save class="size-4".to_string()/> }.into_any()
                    }}
                    {move || if state.mutation_status.get().is_pending() { i18n.t(T::Saving) } else { i18n.t(T::SavePermissions) }}
                </Button>
            </DialogFooter>
        </CrudDialog>
    }
}
