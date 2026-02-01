FROM rust:1.93

WORKDIR /usr/src/ctf-operator
COPY . .

RUN cargo build --release

CMD ["/usr/src/ctf-operator/target/release/ctf-operator"]
