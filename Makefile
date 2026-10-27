# VulnFeed Makefile — make help 查看全部目标
CARGO    ?= cargo
ARCH     ?= $(shell uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/')
TRIPLE_arm64  := aarch64-unknown-linux-gnu
TRIPLE_aarch64 := aarch64-unknown-linux-gnu
TRIPLE_amd64  := x86_64-unknown-linux-gnu
TRIPLE_x86_64 := x86_64-unknown-linux-gnu
ZIGTRIPLE ?= $(TRIPLE_$(ARCH))
ZIG_TARGETS ?= x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
BUN      ?= bun
BIN      ?= vulnfeed
VERSION  ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo dev)
DIST     ?= dist

.DEFAULT_GOAL := help
.PHONY: help check clippy fmt fmt-check test run userpwd build release package \
        zig-build zig-package docker-zig docker clean

help: ## 显示所有可用目标
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

check: ## cargo check --all
	$(CARGO) check --all

clippy: ## clippy(CI 门禁,-D warnings)
	$(CARGO) clippy --all-targets --all-features --tests --benches -- -D warnings

fmt: ## 格式化代码
	$(CARGO) fmt

fmt-check: ## 格式检查(CI 用)
	$(CARGO) fmt --check

test: ## 全量测试(优先 nextest,未安装则回退 cargo test)
	$(CARGO) nextest run --all-features || $(CARGO) test

run: ## 启动服务(可省略 --config-file,内置默认配置等价 dev/config.toml)
	$(CARGO) run --bin $(BIN) server

userpwd: ## 修改超管密码: make userpwd PASSWORD='xxx'
	@test -n "$(PASSWORD)" || { echo "用法: make userpwd PASSWORD='<新密码>'"; exit 1; }
	$(CARGO) run --bin $(BIN) change-super-user-password -p "$(PASSWORD)"

build: ## 调试构建(前端自动嵌入)
	$(CARGO) build

release: ## 发布构建
	$(CARGO) build --release

package: release ## 打包:release 二进制 + 默认配置 -> dist/vulnfeed-<version>.tar.gz
	@mkdir -p $(DIST)/vulnfeed-$(VERSION)
	@cp target/release/$(BIN) $(DIST)/vulnfeed-$(VERSION)/
	@cp dev/config.toml $(DIST)/vulnfeed-$(VERSION)/config.toml
	@tar -czf $(DIST)/vulnfeed-$(VERSION).tar.gz -C $(DIST) vulnfeed-$(VERSION)
	@echo "打包完成: $(DIST)/vulnfeed-$(VERSION).tar.gz"

docker: ## 构建 Docker 镜像
	docker build --push -t vulnfeed:latest .

clean: ## cargo clean
	$(CARGO) clean

ZIG_REQ = @command -v cargo-zigbuild >/dev/null 2>&1 || { echo "需要先安装: cargo install cargo-zigbuild && pip3 install ziglang (或安装 zig)"; exit 1; }

zig-build: ## cargo-zigbuild 跨平台本地构建(目标见 ZIG_TARGETS)
	$(ZIG_REQ)
	@for t in $(ZIG_TARGETS); do \
		rustup target list --installed | grep -q "^$$t$$" || rustup target add $$t; \
		echo "==> zigbuild $$t"; \
		cargo zigbuild --release --target $$t; \
	done

zig-package: zig-build ## 跨平台打包: dist/vulnfeed-<version>-<target>.tar.gz
	@for t in $(ZIG_TARGETS); do \
		mkdir -p $(DIST)/vulnfeed-$(VERSION)-$$t; \
		cp target/$$t/release/$(BIN) $(DIST)/vulnfeed-$(VERSION)-$$t/; \
		cp dev/config.toml $(DIST)/vulnfeed-$(VERSION)-$$t/config.toml; \
		tar -czf $(DIST)/vulnfeed-$(VERSION)-$$t.tar.gz -C $(DIST) vulnfeed-$(VERSION)-$$t; \
		rm -rf $(DIST)/vulnfeed-$(VERSION)-$$t; \
		echo "打包完成: $(DIST)/vulnfeed-$(VERSION)-$$t.tar.gz"; \
	done

docker-zig: zig-build ## 本地 zigbuild 二进制打入镜像: make docker-zig [ARCH=amd64|arm64]
	$(ZIG_REQ)
	@command -v docker >/dev/null 2>&1 || { echo "需要 docker"; exit 1; }
	@mkdir -p dist/docker/$(ZIGTRIPLE)
	@cp target/$(ZIGTRIPLE)/release/$(BIN) dist/docker/$(ZIGTRIPLE)/
	@cp dev/config.toml dist/docker/$(ZIGTRIPLE)/config.toml
	docker build -f Dockerfile.zig --build-arg TARGET_TRIPLE=$(ZIGTRIPLE) -t vulnfeed:$(ARCH)-zig .
	@rm -rf dist/docker
	@echo "镜像完成: vulnfeed:$(ARCH)-zig"
