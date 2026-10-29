# 将本地 cargo-zigbuild 产物直接打入精简镜像(buildx 多平台)
# 构建: make docker(单架构) / IMAGE=xxx make docker-multi(多平台推送)
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
ARG TARGETPLATFORM
ENV TZ=Asia/Shanghai
WORKDIR /app
COPY dist/docker/${TARGETPLATFORM}/vulnfeed /usr/local/bin/vulnfeed
COPY dist/docker/${TARGETPLATFORM}/config.toml /app/config.toml
RUN chmod +x /usr/local/bin/vulnfeed && mkdir -p /app/data /app/logs
EXPOSE 9000
VOLUME ["/app/data", "/app/logs"]
ENTRYPOINT ["vulnfeed"]
CMD ["server"]
