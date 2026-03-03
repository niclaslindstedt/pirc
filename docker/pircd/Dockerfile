FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates curl netcat-openbsd python3 \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /data /opt/pirc /cluster-init

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 6667 6668

ENTRYPOINT ["/entrypoint.sh"]
