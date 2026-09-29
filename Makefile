# easyS3 构建脚本（兼容 Windows cmd.exe 与 POSIX shell）
.DEFAULT_GOAL := help

PNPM ?= pnpm
CARGO ?= cargo
VERSION ?=
TARGET ?=

.PHONY: help install dev tauri-dev build frontend-build core-build version release \
	check test fmt fmt-check clippy verify clean

help: ## 显示常用目标
	@echo 常用目标：
	@echo   make install          安装前端依赖
	@echo   make dev              启动前端开发服务器
	@echo   make tauri-dev        启动 Tauri 桌面端开发模式
	@echo   make build            构建前端和核心 Rust 库
	@echo   make version          更新版本号，例如 make version VERSION=1.2.3
	@echo   make release          构建当前平台的 Tauri 发布包
	@echo   make release VERSION=1.2.3 TARGET=x86_64-pc-windows-msvc
	@echo   make check            检查 Rust 工作区
	@echo   make test             运行核心库测试
	@echo   make fmt              格式化 Rust 代码
	@echo   make fmt-check        检查 Rust 代码格式
	@echo   make clippy           运行核心库 Clippy 检查
	@echo   make verify           执行提交前完整检查
	@echo   make clean            清理构建产物

install: ## 安装前端依赖
	$(PNPM) install --frozen-lockfile

dev: ## 启动前端开发服务器
	$(PNPM) dev

tauri-dev: ## 启动 Tauri 桌面端开发模式
	$(PNPM) tauri dev

frontend-build: ## 构建前端资源
	$(PNPM) build

core-build: ## 构建 easys3-core 发布版本
ifneq ($(TARGET),)
	$(CARGO) build -p easys3-core --release --target $(TARGET)
else
	$(CARGO) build -p easys3-core --release
endif

build: frontend-build core-build ## 构建前端和核心 Rust 库

version: ## 更新版本号，用法 make version VERSION=1.2.3
	@node scripts/set-version.mjs $(VERSION)

ifneq ($(VERSION),)
release: version
endif

release: ## 构建发布包，可指定 VERSION 与 TARGET
ifneq ($(TARGET),)
	$(PNPM) tauri build --target $(TARGET)
else
	$(PNPM) tauri build
endif

check: ## 检查 Rust 工作区
	$(CARGO) check --workspace

test: ## 运行核心库测试
	$(CARGO) test -p easys3-core

fmt: ## 格式化 Rust 代码
	$(CARGO) fmt --all

fmt-check: ## 检查 Rust 代码格式
	$(CARGO) fmt --all -- --check

clippy: ## 运行核心库 Clippy 检查
	$(CARGO) clippy -p easys3-core --all-targets -- -D warnings

verify: frontend-build fmt-check clippy test ## 执行提交前完整检查

clean: ## 清理 Rust 编译缓存和前端构建产物
	$(CARGO) clean
	node -e "require('node:fs').rmSync('dist',{recursive:true,force:true})"
