FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y socat coreutils procps && rm -rf /var/lib/apt/lists/*

WORKDIR /home/ctf

EXPOSE 1337

CMD ["socat", "TCP-LISTEN:1337,reuseaddr,fork", "EXEC:/bin/bash,pty,stderr,setsid,sigint,sane"]
