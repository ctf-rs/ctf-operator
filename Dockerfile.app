FROM rust:1.95 AS builder

RUN rustup target add wasm32-unknown-unknown \
    && cargo install cargo-leptos

WORKDIR /usr/src/ctf-operator

COPY Cargo.toml ./
COPY app/Cargo.toml ./app/Cargo.toml
COPY bot/Cargo.toml ./bot/Cargo.toml
COPY cli/Cargo.toml ./cli/Cargo.toml
COPY operator/Cargo.toml ./operator/Cargo.toml

RUN mkdir -p app/src bot/src cli/src operator/src \
    && printf 'fn main() {}\n' > app/src/main.rs \
    && printf 'pub fn placeholder() {}\n' > app/src/lib.rs \
    && printf 'fn main() {}\n' > bot/src/main.rs \
    && printf 'pub fn placeholder() {}\n' > bot/src/lib.rs \
    && printf 'fn main() {}\n' > cli/src/main.rs \
    && printf 'pub fn placeholder() {}\n' > cli/src/lib.rs \
    && printf 'fn main() {}\n' > operator/src/main.rs \
    && printf 'pub fn placeholder() {}\n' > operator/src/lib.rs

RUN cargo fetch

COPY cli/src ./cli/src
COPY app/src ./app/src

RUN cargo leptos build --release

FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/ctf-operator/target/release/app /usr/local/bin/app
COPY --from=builder /usr/src/ctf-operator/target/site /app/target/site

WORKDIR /app

ENTRYPOINT ["/usr/local/bin/app"]