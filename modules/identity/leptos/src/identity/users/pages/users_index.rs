use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::{
    domain_shared::identity::is_protected_admin_username,
    web::{
        app::{layout::WorkspaceRouteLayout, page::PageSection},
        identity::{
            roles::server_fns::list_all_roles_admin,
            users::{
                components::{
                    user_form::UserFormPanel, users_filters::UsersFilters,
                    users_page_header::UsersPageHeader, users_table::UsersTable,
                },
                model::users_page_state::{UserSaveInput, UsersPageState},
                server_fns::{
                    create_user_admin, delete_user_admin, get_user, list_users, update_user_admin,
                },
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
pub fn UsersIndexRoute() -> impl IntoView {
    let state = UsersPageState::new();
    let toast = use_toast();
    let i18n = use_i18n();

    let load_users = Callback::new({
        move |()| {
            let input = state.list_input();

            state.loading.set(true);
            state.error.set(None);

            spawn_local(async move {
                let users_result = list_users(input).await;
                let roles_result = list_all_roles_admin().await;

                match (users_result, roles_result) {
                    (Ok(result), Ok(roles)) => {
                        state.users.set(result.items);
                        state.total_count.set(result.total_count);
                        state.page.set(result.page);
                        state.roles.set(roles);
                    }
                    (Err(err), _) | (_, Err(err)) => state.error.set(Some(err.to_string())),
                }
                state.loading.set(false);
            });
        }
    });

    Effect::new(move |_| load_users.run(()));

    let open_create_form = Callback::new(move |()| state.open_create_form());

    let cancel_form = Callback::new(move |()| state.close_form());

    let edit_user = Callback::new({
        let toast = toast.clone();
        move |user: application_contracts::identity::users::UserDto| {
            state.form_error.set(None);
            let toast = toast.clone();

            spawn_local(async move {
                match get_user(user.username.clone()).await {
                    Ok(dto) => state.open_edit_form(dto),
                    Err(err) => {
                        let message = err.to_string();
                        state.form_error.set(Some(message.clone()));
                        toast.error(i18n.t_untracked(T::UserSaveFailed), message);
                    }
                }
            });
        }
    });

    let submit_user = Callback::new({
        let toast = toast.clone();
        move |()| {
            if state.save_status.get_untracked().is_pending() {
                return;
            }

            let save_input = match state.save_input(i18n) {
                Ok(input) => input,
                Err(message) => {
                    state.form_error.set(Some(message));
                    return;
                }
            };

            state.save_status.set(MutationStatus::Pending);
            state.form_error.set(None);
            let toast = toast.clone();

            spawn_local(async move {
                let is_editing = matches!(save_input, UserSaveInput::Update(_));
                let result = match save_input {
                    UserSaveInput::Create(input) => create_user_admin(input).await.map(|_| ()),
                    UserSaveInput::Update(input) => update_user_admin(input).await.map(|_| ()),
                };

                match result {
                    Ok(()) => {
                        state.form_open.set(false);
                        state.save_status.set(MutationStatus::Success);
                        if is_editing {
                            toast.success(
                                i18n.t_untracked(T::UserUpdated),
                                i18n.t_untracked(T::UserUpdatedDescription),
                            );
                        } else {
                            toast.success(
                                i18n.t_untracked(T::UserCreated),
                                i18n.t_untracked(T::UserCreatedDescription),
                            );
                        }
                        load_users.run(());
                    }
                    Err(err) => {
                        let message = err.to_string();
                        state
                            .save_status
                            .set(MutationStatus::Failed(message.clone()));
                        state.form_error.set(Some(message.clone()));
                        toast.error(i18n.t_untracked(T::UserSaveFailed), message);
                    }
                }
                if state.save_status.get_untracked().is_pending() {
                    state.save_status.set(MutationStatus::Idle);
                }
            });
        }
    });

    let delete_user = Callback::new({
        let toast = toast.clone();
        move |target_username: String| {
            if is_protected_admin_username(&target_username) {
                toast.error(
                    i18n.t_untracked(T::UserDeleteFailed),
                    i18n.t_untracked(T::ProtectedAdminCannotBeDeleted),
                );
                return;
            }

            state.deleting_username.set(Some(target_username.clone()));
            state.delete_status.set(MutationStatus::Pending);
            state.error.set(None);
            let toast = toast.clone();

            spawn_local(async move {
                match delete_user_admin(target_username.clone()).await {
                    Ok(()) => {
                        state.delete_status.set(MutationStatus::Success);
                        toast.success(
                            i18n.t_untracked(T::UserDeleted),
                            format!("{target_username} was removed."),
                        );
                        load_users.run(());
                    }
                    Err(err) => {
                        let message = err.to_string();
                        state
                            .delete_status
                            .set(MutationStatus::Failed(message.clone()));
                        state.error.set(Some(message.clone()));
                        toast.error(i18n.t_untracked(T::UserDeleteFailed), message);
                    }
                }
                state.deleting_username.set(None);
                if state.delete_status.get_untracked().is_pending() {
                    state.delete_status.set(MutationStatus::Idle);
                }
            });
        }
    });

    let run_search = Callback::new({
        move |()| {
            state.page.set(1);
            load_users.run(());
        }
    });

    let reset_filters = Callback::new({
        move |()| {
            state.reset_filters();
            load_users.run(());
        }
    });

    let go_to_page = Callback::new({
        move |target_page: u32| {
            state.page.set(target_page.max(1));
            load_users.run(());
        }
    });

    view! {
        <WorkspaceRouteLayout title=T::Users>
            <div class="page-stack">
                <UsersPageHeader on_new=open_create_form/>

                <PageSection class="grid gap-4">
                    <UsersFilters
                        state=state
                        on_search=run_search
                        on_reset=reset_filters
                    />
                    <UserFormPanel
                        state=state
                        on_cancel=cancel_form
                        on_submit=submit_user
                    />
                    <UsersTable
                        state=state
                        on_edit=edit_user
                        on_delete=delete_user
                        on_page_change=go_to_page
                    />
                </PageSection>
            </div>
        </WorkspaceRouteLayout>
    }
}
