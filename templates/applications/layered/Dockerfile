FROM node:26-bookworm-slim AS node

FROM rust:1.97.0-bookworm AS builder

WORKDIR /app
COPY --from=node /usr/local /usr/local
RUN rustup target add wasm32-unknown-unknown && \
    cargo install --locked cargo-leptos --version 0.3.7
COPY . .
RUN npm ci --prefix apps/web/src && \
    PATH="/app/apps/web/src/node_modules/.bin:${PATH}" \
      cargo leptos build -p app_server --release \
      --bin-features ssr,db-postgres --lib-features hydrate

FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/app_server /usr/local/bin/app_server
COPY --from=builder /app/target/site /app/site
COPY --from=builder /app/config/production.yaml /app/config/production.yaml

WORKDIR /app
ENV APP_ENV=production
ENV LEPTOS_SITE_ROOT=site
ENV LEPTOS_SITE_ADDR=0.0.0.0:3000
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl --fail http://localhost:3000/healthz || exit 1
ENTRYPOINT ["app_server"]
