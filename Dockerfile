# 将本地 cargo-zigbuild 产物直接打入精简镜像(buildx 多平台)
# 构建: make docker(单架构) / IMAGE=xxx make docker-multi(多平台推送)
ARG TARGETPLATFORM=linux/arm64
FROM debian:bookworm-slim
ARG TARGETPLATFORM
ENV TZ=Asia/Shanghai
WORKDIR /app
COPY dist/docker/${TARGETPLATFORM}/vulnfeed /app/vulnfeed
COPY dist/docker/${TARGETPLATFORM}/config.toml /app/config.toml
RUN chmod +x /app/vulnfeed && mkdir -p /app/data /app/logs
EXPOSE 9000
VOLUME ["/app/data", "/app/logs"]
ENTRYPOINT ["/app/vulnfeed"]
CMD ["server"]
