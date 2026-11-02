FROM rust:1.95 AS builder

WORKDIR /usr/src/ctf-operator

COPY Cargo.toml ./
COPY Cargo.lock ./
COPY cli/Cargo.toml ./cli/Cargo.toml
COPY core/Cargo.toml ./core/Cargo.toml
COPY operator/Cargo.toml ./operator/Cargo.toml

RUN mkdir -p cli/src core/src operator/src \
    && printf 'fn main() {}\n' > cli/src/main.rs \
    && printf 'pub fn placeholder() {}\n' > cli/src/lib.rs \
    && printf 'pub fn placeholder() {}\n' > core/src/lib.rs \
    && printf 'fn main() {}\n' > operator/src/main.rs \
    && printf 'pub fn placeholder() {}\n' > operator/src/lib.rs

RUN cargo fetch --locked

COPY cli/src ./cli/src
COPY core/src ./core/src
COPY operator/src ./operator/src

RUN cargo build --locked --release -p ctf-operator --bin ctf-operator

FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/ctf-operator/target/release/ctf-operator /usr/local/bin/ctf-operator

ENTRYPOINT ["/usr/local/bin/ctf-operator"]
CMD ["operator"]