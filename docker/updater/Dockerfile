FROM alpine:3.21
RUN apk add --no-cache bash curl python3 docker-cli jq
COPY updater.sh /updater.sh
RUN chmod +x /updater.sh
CMD ["/updater.sh"]
