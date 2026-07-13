use icons::{ChevronLeft, ChevronRight, Users};
use leptos::prelude::*;

use crate::{
    application_contracts::identity::users::UserDto,
    web::{
        identity::users::components::user_row_actions::{DeleteUserDialog, UserRowActions},
        identity::users::model::users_page_state::{USERS_PAGE_SIZE, UsersPageState},
        shared::{
            i18n::{T, use_i18n},
            rust_ui::ui::{
                alert::{Alert, AlertDescription},
                avatar::{Avatar, AvatarFallback},
                badge::{Badge, BadgeVariant},
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
pub fn UsersTable(
    state: UsersPageState,
    on_edit: Callback<UserDto>,
    on_delete: Callback<String>,
    on_page_change: Callback<u32>,
) -> impl IntoView {
    let i18n = use_i18n();
    let confirm_delete_username = RwSignal::new(Option::<String>::None);
    let range_label = move || {
        let total = state.total_count.get();
        let filtered_count =
            filtered_users(state.users.get(), state.verification.get()).len() as i64;
        if total == 0 || filtered_count == 0 {
            format!("0 {}", i18n.t(T::Users).to_lowercase())
        } else {
            let start = ((state.page.get().saturating_sub(1) * USERS_PAGE_SIZE) + 1) as i64;
            let end = (start + filtered_count - 1).min(total);
            format!(
                "{start}-{end} of {total} {}",
                i18n.t(T::Users).to_lowercase()
            )
        }
    };

    view! {
        <div class="grid gap-3">
            {move || {
                state
                    .error
                    .get()
                    .map(|message| {
                        view! {
                            <Alert class="border-destructive/30 bg-destructive/5 text-destructive".to_string()>
                                <AlertDescription>{message}</AlertDescription>
                            </Alert>
                        }
                    })
            }}

            <TableWrapper class="max-h-none border-border".to_string()>
                <Table class="min-w-[44rem] max-w-none".to_string()>
                    <TableHeader>
                        <TableRow>
                            <TableHead class="w-28".to_string()>{move || i18n.t(T::Action)}</TableHead>
                            <TableHead class="w-full".to_string()>{move || i18n.t(T::User)}</TableHead>
                            <TableHead class="w-36".to_string()>{move || i18n.t(T::Status)}</TableHead>
                            <TableHead class="w-44".to_string()>{move || i18n.t(T::Created)}</TableHead>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {move || {
                            let rows = filtered_users(state.users.get(), state.verification.get());
                            if state.loading.get() && rows.is_empty() {
                                (0..5)
                                    .map(|_| {
                                        view! {
                                            <TableRow>
                                                <TableCell class="w-28".to_string()>
                                                    <Skeleton class="h-8 w-8".to_string()/>
                                                </TableCell>
                                                <TableCell><Skeleton class="h-8 w-44".to_string()/></TableCell>
                                                <TableCell><Skeleton class="h-6 w-20".to_string()/></TableCell>
                                                <TableCell><Skeleton class="h-5 w-32".to_string()/></TableCell>
                                            </TableRow>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            } else if rows.is_empty() {
                                view! {
                                    <TableRow>
                                        <TableCell class="h-32 text-center text-muted-foreground".to_string() attr:colspan="4">
                                            <Empty class="border-0 p-6".to_string()>
                                                <EmptyMedia variant=EmptyMediaVariant::Icon>
                                                    <Users class="size-5".to_string()/>
                                                </EmptyMedia>
                                                <EmptyHeader>
                                                    <EmptyTitle>{move || i18n.t(T::NoUsersFound)}</EmptyTitle>
                                                    <EmptyDescription>
                                                        {move || i18n.t(T::NoUsersFoundDescription)}
                                                    </EmptyDescription>
                                                </EmptyHeader>
                                            </Empty>
                                        </TableCell>
                                    </TableRow>
                                }
                                    .into_any()
                            } else {
                                rows
                                    .into_iter()
                                    .map(|user| {
                                        let action_user = user.clone();
                                        let username_label = user.username.clone();
                                        let username_initial = user.username.chars().next().unwrap_or('U').to_uppercase().to_string();
                                        view! {
                                            <TableRow>
                                                <TableCell class="w-28".to_string()>
                                                    <UserRowActions
                                                        user=action_user
                                                        confirm_delete_username=confirm_delete_username
                                                        on_edit=on_edit
                                                    />
                                                </TableCell>
                                                <TableCell>
                                                    <div class="flex items-center gap-3">
                                                        <Avatar>
                                                            <AvatarFallback>
                                                                {username_initial}
                                                            </AvatarFallback>
                                                        </Avatar>
                                                        <div class="grid gap-0.5">
                                                            <strong>{username_label}</strong>
                                                            <small class="text-muted-foreground">{move || i18n.t(T::IdentityUser)}</small>
                                                        </div>
                                                    </div>
                                                </TableCell>
                                                <TableCell>
                                                    {if user.is_verified {
                                                        view! { <Badge variant=BadgeVariant::Success>{move || i18n.t(T::Verified)}</Badge> }.into_any()
                                                    } else {
                                                        view! { <Badge variant=BadgeVariant::Warning>{move || i18n.t(T::Pending)}</Badge> }.into_any()
                                                    }}
                                                </TableCell>
                                                <TableCell>{user.created_at.format("%Y-%m-%d %H:%M").to_string()}</TableCell>
                                            </TableRow>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
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
                    <Pagination attr:aria-label="Users pagination">
                        <PaginationList>
                            <PaginationItem>
                                <PaginationNavButton
                                    disabled=move || !state.can_go_previous()
                                    on_click=Callback::new(move |()| {
                                        on_page_change.run(state.page.get_untracked().saturating_sub(1));
                                    })
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
                                    disabled=move || !state.can_go_next()
                                    on_click=Callback::new(move |()| {
                                        on_page_change.run(state.page.get_untracked() + 1);
                                    })
                                >
                                    {move || i18n.t(T::Next)}
                                    <ChevronRight class="size-4".to_string()/>
                                </PaginationNavButton>
                            </PaginationItem>
                        </PaginationList>
                    </Pagination>
                </div>
            </div>

            <DeleteUserDialog
                state=state
                confirm_delete_username=confirm_delete_username
                on_delete=on_delete
            />
        </div>
    }
}

fn filtered_users(users: Vec<UserDto>, verification: String) -> Vec<UserDto> {
    users
        .into_iter()
        .filter(|user| match verification.as_str() {
            "verified" => user.is_verified,
            "unverified" => !user.is_verified,
            _ => true,
        })
        .collect()
}
