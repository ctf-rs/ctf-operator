FROM rust:1.93 AS builder

WORKDIR /usr/src/ctf-operator

COPY Cargo.toml ./
COPY app/Cargo.toml ./app/Cargo.toml
COPY cli/Cargo.toml ./cli/Cargo.toml
COPY operator/Cargo.toml ./operator/Cargo.toml

RUN mkdir -p app/src cli/src operator/src \
    && printf 'fn main() {}\n' > app/src/main.rs \
    && printf 'fn main() {}\n' > cli/src/main.rs \
    && printf 'pub fn placeholder() {}\n' > cli/src/lib.rs \
    && printf 'fn main() {}\n' > operator/src/main.rs \
    && printf 'pub fn placeholder() {}\n' > operator/src/lib.rs

RUN cargo fetch

COPY cli/src ./cli/src
COPY operator/src ./operator/src

RUN cargo build --release -p ctf-operator --bin ctf-operator

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/ctf-operator/target/release/ctf-operator /usr/local/bin/ctf-operator

ENTRYPOINT ["/usr/local/bin/ctf-operator"]
CMD ["operator"]