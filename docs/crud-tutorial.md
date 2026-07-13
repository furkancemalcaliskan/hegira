# CRUD Tutorial

This tutorial shows how to add a complete CRUD feature without a generator.
The existing `Catalog::Products` feature is the working reference, but the same
sequence applies to other bounded contexts.

The finished feature includes domain rules, PostgreSQL and SQLite persistence,
permissions, transactional audit, Axum JSON endpoints, OpenAPI, a Leptos page,
and provider contract tests.

## 1. Choose The Feature Shape

Start with a bounded context and plural capability name. A standard feature is
kept cohesive instead of creating one file for every DTO or operation:

```text
crates/domain/src/catalog/products.rs
crates/application_contracts/src/catalog/products.rs
crates/application/src/catalog/products.rs
crates/infrastructure/src/catalog/{postgres,sqlite}.rs
crates/presentation/src/http/controllers/catalog/products.rs
crates/web/src/catalog/products/{server_fns,page}.rs
```

Products exposes an internal integer key, public UUID, name, SKU, minor-unit
price, active state, optimistic revision, timestamps, and soft delete.

## 2. Add Both Migrations

Add the next numbered files under:

```text
crates/infrastructure/src/db/migrations/
crates/infrastructure/src/db/migrations_sqlite/
```

Use provider-native types and indexes. Keep equivalent business constraints,
but do not force PostgreSQL and SQLite to share SQL text. Products uses
`021_catalog_products.sql` and `008_catalog_products.sql` as examples.

Never edit a migration that may already have been applied. Verify a fresh
SQLite database immediately:

```sh
ALLOW_DB_RESET=true APP_ENV=sqlite \
cargo run -p db_migrator --no-default-features --features ssr,db-sqlite -- recreate
```

## 3. Define Domain Rules And Persistence Ports

Put the entity, create/update value objects, list query, sort enum, and
repository trait in the domain feature. The Products implementation trims
names, normalizes SKUs, rejects negative prices, and requires a positive
revision for mutations.

The repository trait accepts domain values and returns `DomainError`. It must
not expose SQLx pools, rows, HTTP status codes, or DTOs.

Required operations for a conventional CRUD feature are:

```rust
pub trait ProductRepository: Send + Sync {
    fn list(&self, query: ProductListQuery) -> impl Future<Output = Result<ProductPage, DomainError>> + Send;
    fn find_by_pid(&self, pid: Uuid) -> impl Future<Output = Result<Option<Product>, DomainError>> + Send;
    fn insert(&self, product: NewProduct) -> impl Future<Output = Result<Product, DomainError>> + Send;
    fn update(&self, pid: Uuid, changes: ProductChanges) -> impl Future<Output = Result<Option<Product>, DomainError>> + Send;
    fn soft_delete(&self, pid: Uuid, expected_revision: i64) -> impl Future<Output = Result<bool, DomainError>> + Send;
}
```

Add unit tests for normalization and every invariant.

## 4. Add Application Contracts

Define transport-independent DTOs and inputs in `application_contracts`:

- entity DTO;
- paged result DTO;
- list input with search, page, page size, and a typed sort enum;
- create input;
- update input containing `expected_revision`;
- delete input containing `expected_revision`.

Derive Serde for shared transport contracts. Add Utoipa schemas behind the
`openapi` feature so hydrated browser builds do not pull server documentation
dependencies into WASM.

## 5. Register Permissions

Define read, create, update, and delete permissions in the bounded context.
Catalog Products uses:

```text
Catalog.Products
Catalog.Products.Create
Catalog.Products.Update
Catalog.Products.Delete
```

Add the capability to `application_contracts::features::FEATURES`. Permission
discovery and admin-role seed reconciliation consume this registry. Add visible
labels and stable error messages to both English and Turkish localization
resources.

## 6. Implement The Application Service

The application service is the use-case boundary. It must:

1. Resolve and authorize the actor.
2. Normalize page limits and validate input.
3. Build domain values.
4. Call the repository or transactional mutation writer.
5. Map entities to DTOs.

Declare a `CrudPolicy` for entity name, permissions, and audit action names.
Products uses `CrudExecution` for authorization and `ProductMutationWriter` for
atomic mutation plus audit. Controllers and pages must not reproduce these
checks.

Optimistic update/delete operations pass `expected_revision`. A stale revision
must produce a stable conflict result instead of silently overwriting data.

## 7. Implement PostgreSQL And SQLite Adapters

Implement the same domain port in both provider files. Bind all input values,
whitelist sort variants, exclude soft-deleted rows, and cap page size in the
application service.

Standard writes implement the feature mutation writer so entity and audit rows
commit or roll back together. Do not call a separate audit logger after the
database transaction has committed.

Create one shared behavioral contract test and run it against both adapters.
PostgreSQL destructive tests must remain opt-in through a disposable
`DATABASE_URL`; SQLite contracts run in the normal test matrix.

## 8. Compose The Service

Add the concrete product service type and field to
`presentation/composition/services.rs`, construct it once, and expose the typed
accessor used by Leptos server functions.

`AppServices` owns long-lived pools and adapters. A handler clones lightweight
service handles; it never rebuilds repositories or external clients per
request.

## 9. Add Axum And OpenAPI

Create a thin controller with list, get, create, update, and delete handlers.
Products is mounted at:

```text
GET    /api/catalog/products
POST   /api/catalog/products
GET    /api/catalog/products/{pid}
PUT    /api/catalog/products/{pid}
DELETE /api/catalog/products/{pid}
```

Handlers extract the Bearer token, path/query/body, call the application
service, and map the result through the shared API error contract. Keep the
feature-local `OpenApi` derive beside the handlers and expose it through an
`HttpFeatureDescriptor`.

## 10. Add Leptos Server Functions

Place UI RPC functions beside the feature. Each server function reads the actor
token from the HttpOnly web session and delegates to the same application
service used by Axum.

Never accept or return the actor token through hydrated client arguments. Add
list, get when required by editing, create, update, and delete functions.

## 11. Build The Page

The Products page demonstrates the expected operational UI:

- server-side search, sorting, and pagination;
- loading, empty, and error states;
- permission-gated create/edit/delete controls;
- create/edit dialog and delete confirmation;
- optimistic revision propagation;
- success and failure feedback.

Reuse `CrudListState`, `CrudDialog`, `MutationStatus`, and existing UI
primitives. Feature-specific state should contain business fields and behavior,
not another generic loading/dialog state machine.

Add the route and navigation item explicitly. Route guards improve UX, but the
application service remains the security boundary.

## 12. Verify The Feature

Run the permanent manual-development gate and provider builds:

```sh
./scripts/dx-manual-check.sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Add an Axum integration test that proves authentication, authorization, error
mapping, and the main CRUD lifecycle. Verify OpenAPI contains the feature paths.

## Definition Of Done

- Domain invariants have unit tests.
- Both providers have migrations and satisfy one behavior contract.
- Every protected use case checks permission in the application layer.
- Mutation and audit are one transaction.
- Update and delete enforce optimistic concurrency.
- Axum and Leptos call the same application service.
- OpenAPI, route, navigation, and localization registrations are complete.
- No external service is required for the SQLite development loop.
- Workspace quality gates pass.

Custom workflows can bypass CRUD UI helpers and expose additional typed
application methods. They must not bypass authorization, transaction, or audit
requirements for administrative mutations.
