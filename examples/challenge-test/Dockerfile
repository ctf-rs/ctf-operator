FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y socat && rm -rf /var/lib/apt/lists/*
RUN useradd -m ctf

WORKDIR /home/ctf

EXPOSE 1337

CMD bash -c "exec socat TCP-LISTEN:1337,reuseaddr,fork EXEC:bash,su=ctf"
