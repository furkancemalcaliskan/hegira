use icons::{Package, Pencil, Plus, Save, Search, Trash2};
use leptos::{prelude::*, task::spawn_local};

use crate::{
    app::{layout::WorkspaceRouteLayout, page::PageSection},
    application_contracts::catalog::{
        permissions,
        products::{
            CreateProductInput, ListProductsInput, ProductDto, ProductSortInput, UpdateProductInput,
        },
    },
    catalog::products::server_fns::{
        create_product, delete_product, list_products, update_product,
    },
    shared::{
        authorization,
        crud::{dialog::CrudDialog, state::CrudListState},
        data::mutation::MutationStatus,
        feedback::toast::use_toast,
        rust_ui::ui::{
            alert::{Alert, AlertDescription},
            badge::{Badge, BadgeVariant},
            button::{Button, ButtonSize, ButtonVariant},
            checkbox::Checkbox,
            dialog::DialogFooter,
            input::{Input, InputType},
            label::Label,
            pagination::{
                Pagination, PaginationItem, PaginationLink, PaginationList, PaginationNavButton,
            },
            skeleton::Skeleton,
            table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper},
        },
    },
};

const PRODUCTS_PAGE_SIZE: u32 = 20;

#[derive(Clone, Copy)]
struct ProductsPageState {
    list: CrudListState<ProductDto, ProductSortInput>,
    items: RwSignal<Vec<ProductDto>>,
    total: RwSignal<i64>,
    page: RwSignal<u32>,
    loading: RwSignal<bool>,
    mutation: RwSignal<MutationStatus>,
    form_open: RwSignal<bool>,
    editing: RwSignal<Option<ProductDto>>,
    name: RwSignal<String>,
    sku: RwSignal<String>,
    price: RwSignal<String>,
    active: RwSignal<bool>,
    confirm_delete: RwSignal<Option<ProductDto>>,
}

enum ProductSaveInput {
    Create(CreateProductInput),
    Update(uuid::Uuid, UpdateProductInput),
}

impl ProductsPageState {
    fn new() -> Self {
        let list = CrudListState::new(ProductSortInput::CreatedAtDesc, PRODUCTS_PAGE_SIZE);
        Self {
            list,
            items: list.items,
            total: list.total,
            page: list.page,
            loading: list.loading,
            mutation: list.mutation,
            form_open: RwSignal::new(false),
            editing: RwSignal::new(None),
            name: RwSignal::new(String::new()),
            sku: RwSignal::new(String::new()),
            price: RwSignal::new(String::new()),
            active: RwSignal::new(true),
            confirm_delete: RwSignal::new(None),
        }
    }
    fn list_input(self) -> ListProductsInput {
        let search = self.list.search.get_untracked().trim().to_string();
        ListProductsInput {
            page: self.list.page.get_untracked(),
            page_size: self.list.page_size(),
            search: (!search.is_empty()).then_some(search),
            sorting: Some(self.list.sorting.get_untracked()),
        }
    }
    fn open_create(self) {
        self.editing.set(None);
        self.name.set(String::new());
        self.sku.set(String::new());
        self.price.set(String::new());
        self.active.set(true);
        self.list.error.set(None);
        self.form_open.set(true);
    }
    fn open_edit(self, item: ProductDto) {
        self.name.set(item.name.clone());
        self.sku.set(item.sku.clone());
        self.price.set(item.price_minor.to_string());
        self.active.set(item.is_active);
        self.editing.set(Some(item));
        self.list.error.set(None);
        self.form_open.set(true);
    }
    fn save_input(self) -> Result<ProductSaveInput, String> {
        let name = self.name.get_untracked().trim().to_string();
        let sku = self.sku.get_untracked().trim().to_string();
        if name.is_empty() || sku.is_empty() {
            return Err("Name and SKU are required".into());
        }
        let price_minor = self
            .price
            .get_untracked()
            .parse::<i64>()
            .map_err(|_| "Price must be a whole number in minor units".to_string())?;
        if price_minor < 0 {
            return Err("Price cannot be negative".into());
        }
        Ok(match self.editing.get_untracked() {
            Some(item) => ProductSaveInput::Update(
                item.pid,
                UpdateProductInput {
                    name,
                    sku,
                    price_minor,
                    is_active: self.active.get_untracked(),
                    expected_revision: item.revision,
                },
            ),
            None => ProductSaveInput::Create(CreateProductInput {
                name,
                sku,
                price_minor,
                is_active: self.active.get_untracked(),
            }),
        })
    }
    fn total_pages(self) -> u32 {
        self.list.total_pages()
    }
}

#[component]
pub fn ProductsIndexRoute() -> impl IntoView {
    let state = ProductsPageState::new();
    let toast = use_toast();
    let load = Callback::new(move |()| {
        state.list.begin_load();
        spawn_local(async move {
            match list_products(state.list_input()).await {
                Ok(result) => {
                    state.list.items.set(result.items);
                    state.list.total.set(result.total_count);
                    state.list.page.set(result.page);
                }
                Err(error) => state.list.error.set(Some(error.to_string())),
            }
            state.list.loading.set(false);
        });
    });
    Effect::new(move |_| load.run(()));

    let save = Callback::new({
        let toast = toast.clone();
        move |()| {
            if state.list.mutation.get_untracked().is_pending() {
                return;
            }
            let input = match state.save_input() {
                Ok(input) => input,
                Err(error) => {
                    state.list.error.set(Some(error));
                    return;
                }
            };
            state.list.mutation.set(MutationStatus::Pending);
            state.list.error.set(None);
            let toast = toast.clone();
            spawn_local(async move {
                let result = match input {
                    ProductSaveInput::Create(input) => create_product(input).await,
                    ProductSaveInput::Update(pid, input) => update_product(pid, input).await,
                };
                match result {
                    Ok(_) => {
                        state.form_open.set(false);
                        state.list.mutation.set(MutationStatus::Success);
                        toast.success("Product saved", "Catalog data is up to date");
                        load.run(());
                    }
                    Err(error) => {
                        let message = error.to_string();
                        state.list.fail(message);
                    }
                }
            });
        }
    });
    let remove = Callback::new({
        let toast = toast.clone();
        move |product: ProductDto| {
            state.list.mutation.set(MutationStatus::Pending);
            let toast = toast.clone();
            spawn_local(async move {
                match delete_product(product.pid, product.revision).await {
                    Ok(()) => {
                        state.confirm_delete.set(None);
                        state.list.mutation.set(MutationStatus::Success);
                        toast.success("Product deleted", product.name);
                        load.run(());
                    }
                    Err(error) => {
                        let message = error.to_string();
                        state.list.fail(message);
                    }
                }
            });
        }
    });

    view! {
        <WorkspaceRouteLayout title=crate::shared::i18n::T::Products>
            <div class="page-stack">
                <header class="page-header">
                    <div class="min-w-0"><h1>"Products"</h1><p>"Catalog inventory and pricing"</p></div>
                    {authorization::can_access_untracked(Some(permissions::PRODUCTS_CREATE)).then(|| view! { <Button on:click=move |_| state.open_create()><Plus class="size-4"/>"New product"</Button> })}
                </header>
                <PageSection class="grid gap-4">
                    <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_13rem_auto]">
                        <div class="relative"><Search class="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"/><Input class="pl-9" r#type=InputType::Search placeholder="Search name or SKU" bind_value=state.list.search/></div>
                        <select class="h-9 rounded-md border bg-background px-3 text-sm" on:change=move |ev| state.list.sorting.set(parse_sort(&event_target_value(&ev)))>
                            <option value="created_at_desc">"Newest"</option><option value="name_asc">"Name A-Z"</option><option value="name_desc">"Name Z-A"</option><option value="price_asc">"Price low-high"</option><option value="price_desc">"Price high-low"</option>
                        </select>
                        <Button variant=ButtonVariant::Outline on:click=move |_| { state.list.page.set(1); load.run(()); }>"Apply"</Button>
                    </div>
                    {move || state.list.error.get().map(|message| view! { <Alert class="border-destructive/30 bg-destructive/5 text-destructive".to_string()><AlertDescription>{message}</AlertDescription></Alert> })}
                    <ProductsTable state=state on_edit=Callback::new(move |item| state.open_edit(item)) on_delete=Callback::new(move |item| state.confirm_delete.set(Some(item))) on_page=Callback::new(move |page| { state.list.page.set(page); load.run(()); })/>
                </PageSection>
            </div>
            <ProductForm state=state on_save=save/>
            <DeleteProductDialog state=state on_delete=remove/>
        </WorkspaceRouteLayout>
    }
}

#[component]
fn ProductsTable(
    state: ProductsPageState,
    on_edit: Callback<ProductDto>,
    on_delete: Callback<ProductDto>,
    on_page: Callback<u32>,
) -> impl IntoView {
    let previous_disabled =
        Signal::derive(move || state.list.page.get() <= 1 || state.list.loading.get());
    let next_disabled = Signal::derive(move || {
        state.list.page.get() >= state.list.total_pages() || state.list.loading.get()
    });
    let previous =
        Callback::new(move |()| on_page.run(state.list.page.get_untracked().saturating_sub(1)));
    let next = Callback::new(move |()| on_page.run(state.list.page.get_untracked() + 1));
    view! { <div class="grid gap-3"><TableWrapper class="max-h-none border-border".to_string()><Table class="min-w-[48rem] max-w-none".to_string()><TableHeader><TableRow><TableHead class="w-28".to_string()>"Actions"</TableHead><TableHead class="w-full".to_string()>"Product"</TableHead><TableHead class="w-36".to_string()>"SKU"</TableHead><TableHead class="w-36".to_string()>"Price"</TableHead><TableHead class="w-28".to_string()>"Status"</TableHead></TableRow></TableHeader><TableBody>{move || { let items = state.items.get(); if state.loading.get() && items.is_empty() { (0..5).map(|_| view! { <TableRow><TableCell attr:colspan="5"><Skeleton class="h-8 w-full".to_string()/></TableCell></TableRow> }).collect_view().into_any() } else if items.is_empty() { view! { <TableRow><TableCell class="h-36 text-center text-muted-foreground".to_string() attr:colspan="5"><Package class="mx-auto mb-2 size-6"/>"No products found"</TableCell></TableRow> }.into_any() } else { items.into_iter().map(|item| { let edit=item.clone(); let delete=item.clone(); view! { <TableRow><TableCell><div class="flex gap-1">{authorization::can_access_untracked(Some(permissions::PRODUCTS_UPDATE)).then(|| view! { <Button size=ButtonSize::Icon variant=ButtonVariant::Ghost attr:title="Edit" on:click=move |_| on_edit.run(edit.clone())><Pencil class="size-4"/></Button> })}{authorization::can_access_untracked(Some(permissions::PRODUCTS_DELETE)).then(|| view! { <Button size=ButtonSize::Icon variant=ButtonVariant::Ghost attr:title="Delete" on:click=move |_| on_delete.run(delete.clone())><Trash2 class="size-4"/></Button> })}</div></TableCell><TableCell><strong>{item.name}</strong></TableCell><TableCell><code>{item.sku}</code></TableCell><TableCell>{format_minor(item.price_minor)}</TableCell><TableCell>{if item.is_active { view!{<Badge variant=BadgeVariant::Success>"Active"</Badge>}.into_any() } else { view!{<Badge>"Inactive"</Badge>}.into_any() }}</TableCell></TableRow> } }).collect_view().into_any() } }}</TableBody></Table></TableWrapper><div class="flex items-center justify-between text-sm text-muted-foreground"><span>{move || format!("{} products", state.total.get())}</span><Pagination><PaginationList><PaginationItem><PaginationNavButton disabled=previous_disabled on_click=previous>"Previous"</PaginationNavButton></PaginationItem><PaginationItem><PaginationLink active=true disabled=true>{move || format!("{} / {}", state.page.get(), state.total_pages())}</PaginationLink></PaginationItem><PaginationItem><PaginationNavButton disabled=next_disabled on_click=next>"Next"</PaginationNavButton></PaginationItem></PaginationList></Pagination></div></div> }
}

#[component]
fn ProductForm(state: ProductsPageState, on_save: Callback<()>) -> impl IntoView {
    let title = Signal::derive(move || {
        if state.editing.get().is_some() {
            "Edit product".to_string()
        } else {
            "Create product".to_string()
        }
    });
    view! { <CrudDialog open=state.form_open title=title description=Signal::derive(move || "Name, SKU, price and availability".to_string()) on_close=Callback::new(move |_| state.form_open.set(false))><form class="grid gap-4" on:submit=move |ev|{ev.prevent_default();on_save.run(());} ><div class="grid gap-2"><Label html_for="product-name">"Name"</Label><Input id="product-name" bind_value=state.name required=true/></div><div class="grid gap-2"><Label html_for="product-sku">"SKU"</Label><Input id="product-sku" bind_value=state.sku required=true/></div><div class="grid gap-2"><Label html_for="product-price">"Price (minor units)"</Label><Input id="product-price" r#type=InputType::Number min="0" step="1" bind_value=state.price required=true/></div><label class="flex items-center gap-3 rounded-md border p-3"><Checkbox checked=Signal::derive(move || state.active.get()) on_checked_change=Callback::new(move |value|state.active.set(value)) aria_label="Active product"/><span>"Active"</span></label><DialogFooter><Button variant=ButtonVariant::Outline on:click=move |ev|{ev.prevent_default();state.form_open.set(false);}>"Cancel"</Button><Button><Save class="size-4"/>{move || if state.mutation.get().is_pending(){"Saving..."}else{"Save"}}</Button></DialogFooter></form></CrudDialog> }
}

#[component]
fn DeleteProductDialog(state: ProductsPageState, on_delete: Callback<ProductDto>) -> impl IntoView {
    let open = RwSignal::new(false);
    Effect::new(move |_| open.set(state.confirm_delete.get().is_some()));
    view! { <CrudDialog open=open title=Signal::derive(move || "Delete product?".to_string()) description=Signal::derive(move || state.confirm_delete.get().map(|p|format!("{} will be removed from active catalog.",p.name)).unwrap_or_default()) on_close=Callback::new(move |_|state.confirm_delete.set(None))><DialogFooter><Button variant=ButtonVariant::Outline on:click=move |_|state.confirm_delete.set(None)>"Cancel"</Button><Button variant=ButtonVariant::Destructive on:click=move |_|if let Some(item)=state.confirm_delete.get_untracked(){on_delete.run(item)}><Trash2 class="size-4"/>"Delete"</Button></DialogFooter></CrudDialog> }
}

fn parse_sort(value: &str) -> ProductSortInput {
    match value {
        "name_asc" => ProductSortInput::NameAsc,
        "name_desc" => ProductSortInput::NameDesc,
        "price_asc" => ProductSortInput::PriceAsc,
        "price_desc" => ProductSortInput::PriceDesc,
        _ => ProductSortInput::CreatedAtDesc,
    }
}
fn format_minor(value: i64) -> String {
    format!("{:.2}", value as f64 / 100.0)
}
