FROM rust:1.93

WORKDIR /usr/src/ctf-operator
COPY . .

RUN cargo build --bin ctf-operator --release 

ENTRYPOINT ["/usr/src/ctf-operator/target/release/ctf-operator"]
CMD ["operator"]
