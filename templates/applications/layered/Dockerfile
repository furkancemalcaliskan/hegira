FROM rust:1.97.0-bookworm AS builder

WORKDIR /app
COPY . .
RUN cargo build --release -p app_server --no-default-features --features db-postgres

FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/app_server /usr/local/bin/app_server
COPY --from=builder /app/config/production.yaml /app/config/production.yaml

WORKDIR /app
ENV APP_ENV=production
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl --fail http://localhost:3000/healthz || exit 1
ENTRYPOINT ["app_server"]

