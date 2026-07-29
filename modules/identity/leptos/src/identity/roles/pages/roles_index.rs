use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::{
    application_contracts::identity::authorization::SetRolePermissionsInput,
    web::{
        app::{layout::WorkspaceRouteLayout, page::PageSection},
        identity::roles::{
            components::{
                role_form::RoleFormPanel, role_permissions_dialog::RolePermissionsDialog,
                roles_filters::RolesFilters, roles_page_header::RolesPageHeader,
                roles_table::RolesTable,
            },
            model::roles_page_state::{RoleSaveInput, RolesPageState},
            server_fns::{
                create_role_admin, delete_role_admin, get_role_admin, list_permissions_admin,
                list_roles_admin, set_role_permissions_admin, update_role_admin,
            },
        },
        shared::{
            feedback::toast::use_toast,
            i18n::{T, use_i18n},
        },
    },
};
use leptos_support::mutation::MutationStatus;

#[component]
pub fn RolesIndexRoute() -> impl IntoView {
    let state = RolesPageState::new();
    let toast = use_toast();
    let i18n = use_i18n();

    let load_roles = Callback::new({
        move |()| {
            let input = state.list_input();
            state.loading.set(true);
            state.error.set(None);

            spawn_local(async move {
                let roles_result = list_roles_admin(input).await;
                let permissions_result = list_permissions_admin().await;

                match (roles_result, permissions_result) {
                    (Ok(result), Ok(permissions)) => {
                        state.roles.set(result.items);
                        state.total_count.set(result.total_count);
                        state.page.set(result.page);
                        state.permissions.set(permissions);
                    }
                    (Err(err), _) | (_, Err(err)) => state.error.set(Some(err.to_string())),
                }

                state.loading.set(false);
            });
        }
    });

    Effect::new(move |_| load_roles.run(()));

    let open_create_form = Callback::new(move |()| state.open_create_form());
    let cancel_form = Callback::new(move |()| state.close_form());
    let edit_role = Callback::new({
        let toast = toast.clone();
        move |role: application_contracts::identity::authorization::RoleDto| {
            state.form_error.set(None);
            let toast = toast.clone();

            spawn_local(async move {
                match get_role_admin(role.name).await {
                    Ok(dto) => state.open_edit_form(dto),
                    Err(err) => {
                        let message = err.to_string();
                        state.form_error.set(Some(message.clone()));
                        toast.error(i18n.t_untracked(T::RoleSaveFailed), message);
                    }
                }
            });
        }
    });
    let permissions_role = Callback::new(move |role| state.open_permissions(role));
    let cancel_permissions = Callback::new(move |()| state.close_permissions());

    let submit_role = Callback::new({
        let toast = toast.clone();
        move |()| {
            if state.mutation_status.get_untracked().is_pending() {
                return;
            }

            let save_input = match state.save_input(i18n) {
                Ok(input) => input,
                Err(message) => {
                    state.form_error.set(Some(message));
                    return;
                }
            };

            state.mutation_status.set(MutationStatus::Pending);
            state.form_error.set(None);
            let toast = toast.clone();

            spawn_local(async move {
                let is_editing = matches!(save_input, RoleSaveInput::Update(_));
                let result = match save_input {
                    RoleSaveInput::Create(input) => create_role_admin(input).await,
                    RoleSaveInput::Update(input) => update_role_admin(input).await,
                };

                match result {
                    Ok(()) => {
                        state.close_form();
                        toast.success(
                            if is_editing {
                                i18n.t_untracked(T::RoleUpdated)
                            } else {
                                i18n.t_untracked(T::RoleCreated)
                            },
                            "",
                        );
                        load_roles.run(());
                    }
                    Err(err) => {
                        let message = err.to_string();
                        state
                            .mutation_status
                            .set(MutationStatus::Failed(message.clone()));
                        state.form_error.set(Some(message.clone()));
                        toast.error(i18n.t_untracked(T::RoleSaveFailed), message);
                    }
                }

                if state.mutation_status.get_untracked().is_pending() {
                    state.mutation_status.set(MutationStatus::Idle);
                }
            });
        }
    });

    let submit_permissions = Callback::new({
        let toast = toast.clone();
        move |()| {
            if state.mutation_status.get_untracked().is_pending() {
                return;
            }

            let Some(role_name) = state.permissions_role.get_untracked() else {
                return;
            };
            let permissions = state.selected_permissions.get_untracked();

            state.mutation_status.set(MutationStatus::Pending);
            let toast = toast.clone();

            spawn_local(async move {
                let result = set_role_permissions_admin(SetRolePermissionsInput {
                    role_name,
                    permissions,
                })
                .await;

                match result {
                    Ok(()) => {
                        state.close_permissions();
                        toast.success(i18n.t_untracked(T::PermissionsSaved), "");
                        load_roles.run(());
                    }
                    Err(err) => {
                        let message = err.to_string();
                        state
                            .mutation_status
                            .set(MutationStatus::Failed(message.clone()));
                        state.error.set(Some(message.clone()));
                        toast.error(i18n.t_untracked(T::PermissionsSaveFailed), message);
                    }
                }

                if state.mutation_status.get_untracked().is_pending() {
                    state.mutation_status.set(MutationStatus::Idle);
                }
            });
        }
    });

    let delete_role = Callback::new({
        let toast = toast.clone();
        move |role_name: String| {
            state.deleting_role.set(Some(role_name.clone()));
            state.error.set(None);
            let toast = toast.clone();

            spawn_local(async move {
                match delete_role_admin(role_name.clone()).await {
                    Ok(()) => {
                        state.deleting_role.set(None);
                        toast.success(i18n.t_untracked(T::RoleDeleted), "");
                        load_roles.run(());
                    }
                    Err(err) => {
                        let message = err.to_string();
                        state.deleting_role.set(None);
                        state.error.set(Some(message.clone()));
                        toast.error(i18n.t_untracked(T::RoleDeleteFailed), message);
                    }
                }
            });
        }
    });

    let run_search = Callback::new(move |()| {
        if !state.loading.get_untracked() {
            state.page.set(1);
            load_roles.run(());
        }
    });

    let reset_filters = Callback::new(move |()| {
        state.reset_filters();
        load_roles.run(());
    });

    let go_to_page = Callback::new(move |target_page: u32| {
        state.page.set(target_page.max(1));
        load_roles.run(());
    });

    view! {
        <WorkspaceRouteLayout title=T::Roles>
            <div class="page-stack">
                <RolesPageHeader on_new=open_create_form/>

                <PageSection class="grid gap-4">
                    <RolesFilters
                        state=state
                        on_search=run_search
                        on_reset=reset_filters
                    />
                    <RoleFormPanel
                        state=state
                        on_cancel=cancel_form
                        on_submit=submit_role
                    />
                    <RolePermissionsDialog
                        state=state
                        on_cancel=cancel_permissions
                        on_submit=submit_permissions
                    />
                    <RolesTable
                        state=state
                        on_edit=edit_role
                        on_permissions=permissions_role
                        on_delete=delete_role
                        on_page_change=go_to_page
                    />
                </PageSection>
            </div>
        </WorkspaceRouteLayout>
    }
}
