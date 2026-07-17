FROM node:22-bookworm-slim AS node-toolchain

FROM rust:1.97.0-bookworm AS chef

COPY --from=node-toolchain /usr/local/ /usr/local/

RUN rustup target add wasm32-unknown-unknown && \
    cargo install --locked cargo-chef --version 0.1.77 && \
    cargo install --locked cargo-leptos --version 0.3.7

WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG SERVER_FEATURES=ssr,db-postgres
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --no-default-features --features ${SERVER_FEATURES} && \
    cargo chef cook --release --recipe-path recipe.json --target wasm32-unknown-unknown --no-default-features --features hydrate
COPY crates/web/src/package.json crates/web/src/package-lock.json crates/web/src/
RUN npm ci --prefix crates/web/src
COPY . .
RUN cargo build --locked --release -p db_migrator --no-default-features --features ssr,db-postgres && \
    cargo leptos build --release --bin-features ${SERVER_FEATURES} --lib-features hydrate

FROM debian:bookworm-slim AS runtime-base

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/hegira /usr/local/bin/hegira
COPY --from=builder /app/target/release/db_migrator /usr/local/bin/db_migrator
COPY --from=builder /app/config/production.yaml /app/config/production.yaml
COPY --from=builder /app/Cargo.toml /app/Cargo.toml

WORKDIR /app
ENTRYPOINT ["hegira"]

FROM runtime-base AS web-runtime

COPY --from=builder /app/target/site /app/site
ENV LEPTOS_SITE_ROOT=site
ENV LEPTOS_SITE_ADDR=0.0.0.0:3000
ENV APP__RUNTIME__ROLE=web

EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/healthz || exit 1
CMD []

FROM runtime-base AS worker-runtime

ENV APP__RUNTIME__ROLE=worker
ENV APP__WORKER_OPERATIONS__ENABLED=true
ENV APP__WORKER_OPERATIONS__ADDR=0.0.0.0:9091
EXPOSE 9091
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:9091/healthz || exit 1
CMD []

FROM web-runtime AS final
