# 将本地 cargo-zigbuild 产物直接打入精简镜像(需先 make zig-build)
# 构建: make docker-zig [ARCH=amd64|arm64]
ARG TARGET_TRIPLE=x86_64-unknown-linux-gnu
FROM debian:bookworm-slim
ARG TARGET_TRIPLE
ENV TZ=Asia/Shanghai
WORKDIR /app
COPY dist/docker/${TARGET_TRIPLE}/vulnfeed /app/vulnfeed
COPY dist/docker/${TARGET_TRIPLE}/config.toml /app/config.toml
RUN chmod +x /app/vulnfeed && mkdir -p /app/data /app/logs
EXPOSE 9000
VOLUME ["/app/data", "/app/logs"]
ENTRYPOINT ["/app/vulnfeed"]
CMD ["server"]
