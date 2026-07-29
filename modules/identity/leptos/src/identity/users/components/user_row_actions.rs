use icons::{Ellipsis, Pencil, Trash2};
use leptos::prelude::*;

use crate::{
    application_contracts::identity::{permissions, users::UserDto},
    domain_shared::identity::is_protected_admin_username,
    web::{
        identity::users::model::users_page_state::UsersPageState,
        shared::{
            authorization,
            i18n::{T, use_i18n},
            rust_ui::ui::{
                button::{Button, ButtonVariant},
                dialog::{DialogBody, DialogDescription, DialogFooter, DialogHeader, DialogTitle},
                dropdown_menu::{
                    DropdownMenu, DropdownMenuAlign, DropdownMenuContent, DropdownMenuItem,
                    DropdownMenuTrigger,
                },
            },
        },
    },
};

#[component]
pub fn UserRowActions(
    user: UserDto,
    confirm_delete_username: RwSignal<Option<String>>,
    on_edit: Callback<UserDto>,
) -> impl IntoView {
    let i18n = use_i18n();
    let edit_user = user.clone();
    let delete_username = user.username.clone();
    let can_update = authorization::can_untracked(permissions::USERS_UPDATE);
    let can_delete = authorization::can_untracked(permissions::USERS_DELETE)
        && !is_protected_admin_username(&user.username);

    view! {
        <DropdownMenu align=DropdownMenuAlign::Start>
            <DropdownMenuTrigger class="size-8 px-0".to_string()>
                <Ellipsis class="size-4".to_string()/>
            </DropdownMenuTrigger>
            <DropdownMenuContent class="w-40".to_string()>
                {can_update.then(|| {
                    view! {
                        <DropdownMenuItem
                            attr:data-dropdown-close="true"
                            on:click=move |_| on_edit.run(edit_user.clone())
                        >
                            <Pencil class="size-4".to_string()/>
                            {move || i18n.t(T::Edit)}
                        </DropdownMenuItem>
                    }
                })}
                {can_delete.then(|| {
                    view! {
                        <DropdownMenuItem
                            attr:data-dropdown-close="true"
                            class="text-destructive hover:bg-destructive/10 hover:text-destructive".to_string()
                            on:click=move |_| {
                                confirm_delete_username.set(Some(delete_username.clone()));
                            }
                        >
                            <Trash2 class="size-4".to_string()/>
                            {move || i18n.t(T::Delete)}
                        </DropdownMenuItem>
                    }
                })}
            </DropdownMenuContent>
        </DropdownMenu>
    }
}

#[component]
pub fn DeleteUserDialog(
    state: UsersPageState,
    confirm_delete_username: RwSignal<Option<String>>,
    on_delete: Callback<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div
            class=move || {
                if confirm_delete_username.get().is_some() {
                    "modal-overlay is-open fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
                } else {
                    "modal-overlay pointer-events-none fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
                }
            }
            aria-hidden=move || confirm_delete_username.get().is_none().to_string()
        >
            <div class=move || {
                if confirm_delete_username.get().is_some() {
                    "modal-panel is-open w-full max-w-md rounded-lg border bg-background p-6 text-foreground shadow-lg"
                } else {
                    "modal-panel w-full max-w-md rounded-lg border bg-background p-6 text-foreground shadow-lg"
                }
            }>
                <DialogBody>
                    <DialogHeader>
                        <DialogTitle>{move || i18n.t(T::DeleteRecord)}</DialogTitle>
                        <DialogDescription>
                            {move || i18n.t(T::DeleteRecordDescriptionPrefix)}
                            <strong>{move || confirm_delete_username.get().unwrap_or_default()}</strong>
                            "."
                        </DialogDescription>
                    </DialogHeader>
                    <DialogFooter>
                        <Button
                            variant=ButtonVariant::Outline
                            on:click=move |_| confirm_delete_username.set(None)
                        >
                            {move || i18n.t(T::Cancel)}
                        </Button>
                        <Button
                            variant=ButtonVariant::Destructive
                            on:click=move |_| {
                                if let Some(username) = confirm_delete_username.get_untracked()
                                    && state.deleting_username.get_untracked().as_ref() != Some(&username)
                                    && !is_protected_admin_username(&username)
                                {
                                    on_delete.run(username);
                                }
                                confirm_delete_username.set(None);
                            }
                        >
                            <Trash2 class="size-4".to_string()/>
                            {move || i18n.t(T::Delete)}
                        </Button>
                    </DialogFooter>
                </DialogBody>
            </div>
        </div>
    }
}
