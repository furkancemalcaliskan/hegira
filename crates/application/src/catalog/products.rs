use application_contracts::catalog::{
    permissions,
    products::{
        CreateProductInput, ListProductsInput, ProductDto, ProductPageDto, ProductSortInput,
        UpdateProductInput,
    },
};
use domain::catalog::products::{
    NewProduct, Product, ProductChanges, ProductListQuery, ProductRepository, ProductSort,
};
use uuid::Uuid;

use crate::{
    identity::authorization::{AuthorizationService, CurrentUserProvider},
    shared::{
        crud::{CrudAuditContext, CrudExecution, CrudPermissions, CrudPolicy},
        errors::{ApplicationError, ApplicationResult},
    },
};

pub const PRODUCT_POLICY: CrudPolicy = CrudPolicy {
    entity_type: "catalog.product",
    permissions: CrudPermissions {
        read: permissions::PRODUCTS,
        create: permissions::PRODUCTS_CREATE,
        update: permissions::PRODUCTS_UPDATE,
        delete: permissions::PRODUCTS_DELETE,
    },
    create_action: "catalog.products.create",
    update_action: "catalog.products.update",
    delete_action: "catalog.products.delete",
};

pub trait ProductMutationWriter: Send + Sync {
    fn create_with_audit(
        &self,
        product: NewProduct,
        audit: CrudAuditContext,
    ) -> impl Future<Output = ApplicationResult<Product>> + Send;
    fn update_with_audit(
        &self,
        pid: Uuid,
        changes: ProductChanges,
        audit: CrudAuditContext,
    ) -> impl Future<Output = ApplicationResult<Option<Product>>> + Send;
    fn delete_with_audit(
        &self,
        pid: Uuid,
        expected_revision: i64,
        audit: CrudAuditContext,
    ) -> impl Future<Output = ApplicationResult<bool>> + Send;
}

#[derive(Debug, Clone)]
pub struct ProductAppService<Products, CurrentUsers, Authorization> {
    products: Products,
    execution: CrudExecution<CurrentUsers, Authorization>,
}

impl<Products, CurrentUsers, Authorization> ProductAppService<Products, CurrentUsers, Authorization>
where
    Products: ProductRepository + ProductMutationWriter,
    CurrentUsers: CurrentUserProvider,
    Authorization: AuthorizationService,
{
    pub fn new(
        products: Products,
        current_users: CurrentUsers,
        authorization: Authorization,
    ) -> Self {
        Self {
            products,
            execution: CrudExecution::new(current_users, authorization),
        }
    }

    pub async fn list(
        &self,
        actor_token: String,
        input: ListProductsInput,
    ) -> ApplicationResult<ProductPageDto> {
        self.execution
            .authorize(&actor_token, PRODUCT_POLICY.permissions.read)
            .await?;
        let page = input.page.max(1);
        let page_size = input.page_size.clamp(1, 100);
        let result = self
            .products
            .list(ProductListQuery {
                page,
                page_size,
                search: input.search.and_then(non_empty),
                sorting: input.sorting.map(product_sort).unwrap_or_default(),
            })
            .await?;
        Ok(ProductPageDto {
            items: result.items.into_iter().map(product_dto).collect(),
            total_count: result.total_count,
            page,
            page_size,
        })
    }

    pub async fn get(&self, actor_token: String, pid: Uuid) -> ApplicationResult<ProductDto> {
        self.execution
            .authorize(&actor_token, PRODUCT_POLICY.permissions.read)
            .await?;
        self.products
            .find_by_pid(pid)
            .await?
            .map(product_dto)
            .ok_or_else(product_not_found)
    }

    pub async fn create(
        &self,
        actor_token: String,
        input: CreateProductInput,
    ) -> ApplicationResult<ProductDto> {
        let actor = self
            .execution
            .authorize(&actor_token, PRODUCT_POLICY.permissions.create)
            .await?;
        let product = NewProduct::validated(
            Uuid::new_v4(),
            input.name,
            input.sku,
            input.price_minor,
            input.is_active,
        )?;
        let product = self
            .products
            .create_with_audit(
                product.clone(),
                audit_context(
                    &actor.username,
                    PRODUCT_POLICY.create_action,
                    product.pid,
                    serde_json::json!({ "sku": product.sku }),
                ),
            )
            .await?;
        Ok(product_dto(product))
    }

    pub async fn update(
        &self,
        actor_token: String,
        pid: Uuid,
        input: UpdateProductInput,
    ) -> ApplicationResult<ProductDto> {
        let actor = self
            .execution
            .authorize(&actor_token, PRODUCT_POLICY.permissions.update)
            .await?;
        let changes = ProductChanges::validated(
            input.name,
            input.sku,
            input.price_minor,
            input.is_active,
            input.expected_revision,
        )?;
        let product = self
            .products
            .update_with_audit(
                pid,
                changes,
                audit_context(
                    &actor.username,
                    PRODUCT_POLICY.update_action,
                    pid,
                    serde_json::json!({ "expected_revision": input.expected_revision }),
                ),
            )
            .await?
            .ok_or_else(product_not_found)?;
        Ok(product_dto(product))
    }

    pub async fn delete(
        &self,
        actor_token: String,
        pid: Uuid,
        expected_revision: i64,
    ) -> ApplicationResult<()> {
        let actor = self
            .execution
            .authorize(&actor_token, PRODUCT_POLICY.permissions.delete)
            .await?;
        if expected_revision < 1 {
            return Err(ApplicationError::Validation(
                "product revision must be greater than zero".to_string(),
            ));
        }
        if !self
            .products
            .delete_with_audit(
                pid,
                expected_revision,
                audit_context(
                    &actor.username,
                    PRODUCT_POLICY.delete_action,
                    pid,
                    serde_json::json!({ "expected_revision": expected_revision }),
                ),
            )
            .await?
        {
            return Err(product_not_found());
        }
        Ok(())
    }
}

fn product_dto(product: Product) -> ProductDto {
    ProductDto {
        pid: product.pid,
        name: product.name,
        sku: product.sku,
        price_minor: product.price_minor,
        is_active: product.is_active,
        revision: product.revision,
        created_at: product.created_at,
        updated_at: product.updated_at,
    }
}
fn product_sort(value: ProductSortInput) -> ProductSort {
    match value {
        ProductSortInput::NameAsc => ProductSort::NameAsc,
        ProductSortInput::NameDesc => ProductSort::NameDesc,
        ProductSortInput::SkuAsc => ProductSort::SkuAsc,
        ProductSortInput::SkuDesc => ProductSort::SkuDesc,
        ProductSortInput::PriceAsc => ProductSort::PriceAsc,
        ProductSortInput::PriceDesc => ProductSort::PriceDesc,
        ProductSortInput::CreatedAtDesc => ProductSort::CreatedAtDesc,
    }
}
fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}
fn product_not_found() -> ApplicationError {
    ApplicationError::NotFound("product not found".to_string())
}

fn audit_context(
    actor: &str,
    action: &'static str,
    pid: Uuid,
    details: serde_json::Value,
) -> CrudAuditContext {
    CrudAuditContext {
        actor: actor.to_string(),
        action,
        entity_type: PRODUCT_POLICY.entity_type,
        entity_id: pid.to_string(),
        details,
    }
}
