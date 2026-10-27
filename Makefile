# VulnFeed Makefile — 容器镜像构建(本地 cargo-zigbuild 产物拷贝式, buildx 多平台)
# 用法: make [docker]              构建当前架构镜像(--load 到本地)
#       make ARCH=amd64            指定架构构建镜像
#       IMAGE=<registry>/<ns>/vulnfeed:<tag> make docker-multi   多平台构建并推送
CARGO    ?= cargo
BIN      ?= vulnfeed
ARCH     ?= $(shell uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/')
PLAT     ?= linux/$(ARCH)
TRIPLE_arm64   := aarch64-unknown-linux-gnu
TRIPLE_aarch64 := aarch64-unknown-linux-gnu
TRIPLE_amd64   := x86_64-unknown-linux-gnu
TRIPLE_x86_64  := x86_64-unknown-linux-gnu
ZIGTRIPLE  ?= $(TRIPLE_$(ARCH))
ZIG_TARGETS ?= x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
IMAGE_REPO ?= 1228022817/vulnfeed
IMAGE_TAG ?= latest
ZIG_REQ = @command -v cargo-zigbuild >/dev/null 2>&1 || { echo "需要先安装: cargo install cargo-zigbuild && pip3 install ziglang (或安装 zig)"; exit 1; }

.DEFAULT_GOAL := docker
.PHONY: docker docker-zig docker-multi zig-build

docker: ## 构建容器镜像(等价 docker-zig)
	@$(MAKE) docker-zig

docker-zig: zig-build ## 构建容器镜像:本地 zigbuild 二进制拷贝进 debian:bookworm-slim
	@command -v docker >/dev/null 2>&1 || { echo "需要 docker"; exit 1; }
	@docker buildx inspect vulnfeed-builder >/dev/null 2>&1 || docker buildx create --name vulnfeed-builder --driver docker-container --use >/dev/null
	@mkdir -p "dist/docker/$(PLAT)"
	@cp target/$(ZIGTRIPLE)/release/$(BIN) "dist/docker/$(PLAT)/"
	@cp dev/config.toml "dist/docker/$(PLAT)/config.toml"
	docker buildx build --platform $(PLAT) -t $(IMAGE_REPO):$(ARCH) --load .
	@rm -rf dist/docker
	@echo "镜像完成: $(IMAGE_REPO):$(ARCH)"

docker-multi: ## buildx 多平台构建并推送: make docker-multi [IMAGE_REPO=...] [IMAGE_TAG=...]
	@command -v docker >/dev/null 2>&1 || { echo "需要 docker"; exit 1; }
	@docker buildx inspect vulnfeed-builder >/dev/null 2>&1 || docker buildx create --name vulnfeed-builder --driver docker-container --use >/dev/null
	$(ZIG_REQ)
	@for t in $(ZIG_TARGETS); do \
		rustup target list --installed | grep -q "^$$t$$" || rustup target add $$t; \
		echo "==> zigbuild $$t"; \
		cargo zigbuild --release --target $$t || exit 1; \
		case $$t in \
			x86_64*) plat=linux/amd64 ;; \
			*) plat=linux/arm64 ;; \
		esac; \
		mkdir -p "dist/docker/$$plat" || exit 1; \
		cp target/$$t/release/$(BIN) "dist/docker/$$plat/" || exit 1; \
		cp dev/config.toml "dist/docker/$$plat/config.toml" || exit 1; \
	done
	docker buildx build --platform linux/amd64,linux/arm64 -t $(IMAGE_REPO):$(IMAGE_TAG) --push .
	@rm -rf dist/docker
	@echo "多平台镜像已推送: $(IMAGE_REPO):$(IMAGE_TAG)"

zig-build: ## 本地 cargo-zigbuild 交叉编译(当前 ZIGTRIPLE)
	$(ZIG_REQ)
	@rustup target list --installed | grep -q "^$(ZIGTRIPLE)$$" || rustup target add $(ZIGTRIPLE)
	cargo zigbuild --release --target $(ZIGTRIPLE)
