# Image produksi backend Rust (APIAMIS). Build context: folder rust/.
# Hanya membangun binary `api`; tidak ada PHP, Composer, atau Node di image ini.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p api && \
    cp target/release/api /usr/local/bin/apiamis

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/apiamis /usr/local/bin/apiamis
USER nobody
ENV APP_PORT=8000
EXPOSE 8000
ENTRYPOINT ["/usr/local/bin/apiamis"]
