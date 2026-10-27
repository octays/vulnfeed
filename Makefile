# VulnFeed Makefile — make help 查看全部目标
CARGO    ?= cargo
BUN      ?= bun
BIN      ?= vulnfeed
VERSION  ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo dev)
DIST     ?= dist

.DEFAULT_GOAL := help
.PHONY: help check clippy fmt fmt-check test run userpwd build release package docker clean

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
	docker build -t vulnfeed:latest .

clean: ## cargo clean
	$(CARGO) clean
