use icons::{ChevronLeft, ChevronRight, Ellipsis, KeyRound, Pencil, Shield, Trash2};
use leptos::prelude::*;

use crate::{
    application_contracts::identity::{authorization::RoleDto, permissions},
    domain_shared::identity::is_protected_admin_role,
    web::{
        identity::roles::model::roles_page_state::{ROLES_PAGE_SIZE, RolesPageState},
        shared::{
            authorization,
            i18n::{T, use_i18n},
            rust_ui::ui::{
                badge::{Badge, BadgeVariant},
                button::{Button, ButtonVariant},
                dialog::{DialogBody, DialogDescription, DialogFooter, DialogHeader, DialogTitle},
                dropdown_menu::{
                    DropdownMenu, DropdownMenuAlign, DropdownMenuContent, DropdownMenuItem,
                    DropdownMenuTrigger,
                },
                empty::{
                    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
                },
                pagination::{
                    Pagination, PaginationItem, PaginationLink, PaginationList, PaginationNavButton,
                },
                skeleton::Skeleton,
                spinner::Spinner,
                table::{
                    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
                },
            },
        },
    },
};

#[component]
pub fn RolesTable(
    state: RolesPageState,
    on_edit: Callback<RoleDto>,
    on_permissions: Callback<RoleDto>,
    on_delete: Callback<String>,
    on_page_change: Callback<u32>,
) -> impl IntoView {
    let i18n = use_i18n();
    let confirm_delete_role = RwSignal::new(Option::<String>::None);
    let range_label = move || {
        let total = state.total_count.get().max(0) as u32;
        let count = state.roles.get().len() as u32;
        if total == 0 || count == 0 {
            format!("0 {}", i18n.t(T::Roles).to_lowercase())
        } else {
            let start = (state.page.get().saturating_sub(1) * ROLES_PAGE_SIZE) + 1;
            let end = (start + count - 1).min(total);
            format!(
                "{start}-{end} of {total} {}",
                i18n.t(T::Roles).to_lowercase()
            )
        }
    };

    view! {
        <div class="grid gap-3">
            {move || {
                state.error.get().map(|message| {
                    view! {
                        <div class="rounded-md border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive">
                            {message}
                        </div>
                    }
                })
            }}

            <TableWrapper class="max-h-none border-border".to_string()>
                <Table class="min-w-[44rem] max-w-none".to_string()>
                    <TableHeader>
                        <TableRow>
                            <TableHead class="w-28".to_string()>{move || i18n.t(T::Action)}</TableHead>
                            <TableHead class="w-full".to_string()>{move || i18n.t(T::Role)}</TableHead>
                            <TableHead class="w-44".to_string()>{move || i18n.t(T::RolePermissions)}</TableHead>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {move || {
                            let rows = state.roles.get();
                            if state.loading.get() && rows.is_empty() {
                                (0..4)
                                    .map(|_| {
                                        view! {
                                            <TableRow>
                                                <TableCell class="w-28".to_string()><Skeleton class="h-8 w-8".to_string()/></TableCell>
                                                <TableCell><Skeleton class="h-8 w-44".to_string()/></TableCell>
                                                <TableCell><Skeleton class="h-6 w-24".to_string()/></TableCell>
                                            </TableRow>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            } else if rows.is_empty() {
                                view! {
                                    <TableRow>
                                        <TableCell class="h-32 text-center text-muted-foreground".to_string() attr:colspan="3">
                                            <Empty class="border-0 p-6".to_string()>
                                                <EmptyMedia variant=EmptyMediaVariant::Icon>
                                                    <Shield class="size-5".to_string()/>
                                                </EmptyMedia>
                                                <EmptyHeader>
                                                    <EmptyTitle>{move || i18n.t(T::NoRolesFound)}</EmptyTitle>
                                                    <EmptyDescription>{move || i18n.t(T::NoRolesFoundDescription)}</EmptyDescription>
                                                </EmptyHeader>
                                            </Empty>
                                        </TableCell>
                                    </TableRow>
                                }.into_any()
                            } else {
                                rows.into_iter().map(|role| {
                                    let edit_role = role.clone();
                                    let permissions_role = role.clone();
                                    let delete_role = role.name.clone();
                                    let can_manage = authorization::can_untracked(permissions::AUTHORIZATION);
                                    let can_delete = can_manage && !is_protected_admin_role(&role.name);
                                    let permission_count = role.permissions.len();
                                    view! {
                                        <TableRow>
                                            <TableCell class="w-28".to_string()>
                                                <DropdownMenu align=DropdownMenuAlign::Start>
                                                    <DropdownMenuTrigger class="size-8 px-0".to_string()>
                                                        <Ellipsis class="size-4".to_string()/>
                                                    </DropdownMenuTrigger>
                                                    <DropdownMenuContent class="w-48".to_string()>
                                                        {can_manage.then(|| {
                                                            view! {
                                                            <DropdownMenuItem attr:data-dropdown-close="true" on:click=move |_| on_edit.run(edit_role.clone())>
                                                                <Pencil class="size-4".to_string()/>{move || i18n.t(T::Edit)}
                                                            </DropdownMenuItem>
                                                            <DropdownMenuItem attr:data-dropdown-close="true" on:click=move |_| on_permissions.run(permissions_role.clone())>
                                                                <KeyRound class="size-4".to_string()/>{move || i18n.t(T::AddPermissions)}
                                                            </DropdownMenuItem>
                                                            }
                                                        })}
                                                            {can_delete.then(|| {
                                                                view! {
                                                                    <DropdownMenuItem
                                                                        attr:data-dropdown-close="true"
                                                                        class="text-destructive hover:bg-destructive/10 hover:text-destructive".to_string()
                                                                        on:click=move |_| confirm_delete_role.set(Some(delete_role.clone()))
                                                                    >
                                                                        <Trash2 class="size-4".to_string()/>{move || i18n.t(T::Delete)}
                                                                    </DropdownMenuItem>
                                                                }
                                                            })}
                                                    </DropdownMenuContent>
                                                </DropdownMenu>
                                            </TableCell>
                                            <TableCell>
                                                <div class="flex items-center gap-3">
                                                    <div class="grid size-9 place-items-center rounded-md bg-muted">
                                                        <Shield class="size-4".to_string()/>
                                                    </div>
                                                    <div class="grid gap-0.5">
                                                        <strong>{role.name}</strong>
                                                        <small class="text-muted-foreground">{move || i18n.t(T::Role)}</small>
                                                    </div>
                                                </div>
                                            </TableCell>
                                            <TableCell>
                                                <Badge variant=BadgeVariant::Secondary>
                                                    {format!("{permission_count}")}
                                                </Badge>
                                            </TableCell>
                                        </TableRow>
                                    }
                                }).collect_view().into_any()
                            }
                        }}
                    </TableBody>
                </Table>
            </TableWrapper>

            <div class="flex flex-col gap-3 text-sm text-muted-foreground sm:flex-row sm:items-center sm:justify-between">
                <div class="inline-flex items-center gap-2">
                    <span>{range_label}</span>
                    {move || state.loading.get().then(|| view! { <><Spinner class="size-3.5".to_string()/>{i18n.t(T::Loading)}</> })}
                </div>
                <div class="inline-flex items-center gap-2 self-end sm:self-auto">
                    <Pagination attr:aria-label="Roles pagination">
                        <PaginationList>
                            <PaginationItem>
                                <PaginationNavButton
                                    disabled=Signal::derive(move || !state.can_go_previous())
                                    on_click=Callback::new(move |()| on_page_change.run(state.page.get_untracked().saturating_sub(1).max(1)))
                                >
                                    <ChevronLeft class="size-4".to_string()/>
                                    {move || i18n.t(T::Previous)}
                                </PaginationNavButton>
                            </PaginationItem>
                            <PaginationItem>
                                <PaginationLink active=true disabled=true class="min-w-20".to_string()>
                                    {move || format!("{} / {}", state.page.get(), state.total_pages())}
                                </PaginationLink>
                            </PaginationItem>
                            <PaginationItem>
                                <PaginationNavButton
                                    disabled=Signal::derive(move || !state.can_go_next())
                                    on_click=Callback::new(move |()| on_page_change.run(state.page.get_untracked() + 1))
                                >
                                    {move || i18n.t(T::Next)}
                                    <ChevronRight class="size-4".to_string()/>
                                </PaginationNavButton>
                            </PaginationItem>
                        </PaginationList>
                    </Pagination>
                </div>
            </div>

            <DeleteRoleDialog
                state=state
                confirm_delete_role=confirm_delete_role
                on_delete=on_delete
            />
        </div>
    }
}

#[component]
fn DeleteRoleDialog(
    state: RolesPageState,
    confirm_delete_role: RwSignal<Option<String>>,
    on_delete: Callback<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div
            class=move || {
                if confirm_delete_role.get().is_some() {
                    "modal-overlay is-open fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
                } else {
                    "modal-overlay pointer-events-none fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
                }
            }
            aria-hidden=move || confirm_delete_role.get().is_none().to_string()
        >
            <div class=move || {
                if confirm_delete_role.get().is_some() {
                    "modal-panel is-open w-full max-w-md rounded-lg border bg-background p-6 text-foreground shadow-lg"
                } else {
                    "modal-panel w-full max-w-md rounded-lg border bg-background p-6 text-foreground shadow-lg"
                }
            }>
                <DialogBody>
                    <DialogHeader>
                        <DialogTitle>{move || i18n.t(T::DeleteRole)}</DialogTitle>
                        <DialogDescription>
                            {move || i18n.t(T::DeleteRoleDescriptionPrefix)}
                            <strong>{move || confirm_delete_role.get().unwrap_or_default()}</strong>
                            "."
                        </DialogDescription>
                    </DialogHeader>
                    <DialogFooter>
                        <Button
                            variant=ButtonVariant::Outline
                            on:click=move |_| confirm_delete_role.set(None)
                        >
                            {move || i18n.t(T::Cancel)}
                        </Button>
                        <Button
                            variant=ButtonVariant::Destructive
                            on:click=move |_| {
                                if let Some(role_name) = confirm_delete_role.get_untracked()
                                    && state.deleting_role.get_untracked().as_ref() != Some(&role_name)
                                    && !is_protected_admin_role(&role_name)
                                {
                                    on_delete.run(role_name);
                                }
                                confirm_delete_role.set(None);
                            }
                        >
                            <Trash2 class="size-4".to_string()/>{move || i18n.t(T::Delete)}
                        </Button>
                    </DialogFooter>
                </DialogBody>
            </div>
        </div>
    }
}
