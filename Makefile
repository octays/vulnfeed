# VulnFeed Makefile — 容器镜像构建(本地 cargo-zigbuild 产物拷贝式)
# 用法: make [docker]              构建当前架构镜像
#       make ARCH=amd64            指定架构构建镜像
CARGO    ?= cargo
BIN      ?= vulnfeed
ARCH     ?= $(shell uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/')
TRIPLE_arm64   := aarch64-unknown-linux-gnu
TRIPLE_aarch64 := aarch64-unknown-linux-gnu
TRIPLE_amd64   := x86_64-unknown-linux-gnu
TRIPLE_x86_64  := x86_64-unknown-linux-gnu
ZIGTRIPLE  ?= $(TRIPLE_$(ARCH))

.DEFAULT_GOAL := docker
.PHONY: docker docker-zig zig-build

docker: ## 构建容器镜像(等价 docker-zig)
	@$(MAKE) docker-zig

docker-zig: zig-build ## 构建容器镜像:本地 zigbuild 二进制拷贝进 debian:bookworm-slim
	@command -v docker >/dev/null 2>&1 || { echo "需要 docker"; exit 1; }
	@mkdir -p dist/docker/$(ZIGTRIPLE)
	@cp target/$(ZIGTRIPLE)/release/$(BIN) dist/docker/$(ZIGTRIPLE)/
	@cp dev/config.toml dist/docker/$(ZIGTRIPLE)/config.toml
	docker build -f Dockerfile --build-arg TARGET_TRIPLE=$(ZIGTRIPLE) -t vulnfeed:$(ARCH)-zig .
	@rm -rf dist/docker
	@echo "镜像完成: vulnfeed:$(ARCH)-zig"

zig-build: ## 本地 cargo-zigbuild 交叉编译(ZIG_TARGETS 可覆盖)
	@command -v cargo-zigbuild >/dev/null 2>&1 || { echo "需要先安装: cargo install cargo-zigbuild && pip3 install ziglang (或安装 zig)"; exit 1; }
	@rustup target list --installed | grep -q "^$(ZIGTRIPLE)$$" || rustup target add $(ZIGTRIPLE)
	cargo zigbuild --release --target $(ZIGTRIPLE)
