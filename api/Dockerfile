# --- build ---
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
# Cache dependencies separately from app code.
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release --locked \
 && rm -rf src target/release/deps/canyon_oaks_api* target/release/canyon-oaks-api*
COPY migrations migrations
COPY src src
RUN touch src/main.rs && cargo build --release --locked

# --- runtime ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* && useradd -r -u 10001 -m app
COPY --from=build /src/target/release/canyon-oaks-api /usr/local/bin/canyon-oaks-api
USER app
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD ["canyon-oaks-api", "healthcheck"]
CMD ["canyon-oaks-api"]
