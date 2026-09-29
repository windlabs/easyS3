# easyS3

> 一个基于 [Tauri 2](https://tauri.app/) 的开源 Amazon S3 协议桌面客户端：面向内部开发、运维与测试人员的对象存储「浏览 / 上传 / 下载 / 删除 / 预览」工具，中文界面，覆盖 Windows / Linux / macOS 三端。

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Tauri 2](https://img.shields.io/badge/Tauri-2.x-24C8DB.svg)](https://tauri.app/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue.svg)](#下载与安装)
[![Rust](https://img.shields.io/badge/Rust-2021-edition-black.svg)](https://www.rust-lang.org/)
[![Vue 3](https://img.shields.io/badge/Vue-3-42b883.svg)](https://vuejs.org/)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

---

## 目录

- [为什么是 easyS3](#为什么是-easys3)
- [功能一览](#功能一览)
- [界面与交互](#界面与交互)
- [下载与安装](#下载与安装)
- [从源码构建](#从源码构建)
- [使用指南](#使用指南)
- [技术架构](#技术架构)
- [项目结构](#项目结构)
- [开发与验证](#开发与验证)
- [范围与非目标](#范围与非目标)
- [参与贡献](#参与贡献)
- [安全](#安全)
- [许可](#许可)

## 为什么是 easyS3

命令行工具（`aws-cli`、`s3cmd`）足够强大，但对非 S3 专家不够直观；网页版控制台通常只服务于公有云，且难以统管自建 / 私有化部署的 S3 兼容服务（MinIO、Ceph、各家对象存储等）。

easyS3 想把「查看桶里有什么、把文件拖进去、把产物拉下来、清理过期对象」这几件日常小事做成一次点击就能完成的桌面体验：

- **贴近业务词汇**：界面只用「项目、桶、文件夹、文件」，不暴露 `prefix`、`multipart` 等协议术语。
- **一套客户端统管多个 S3**：每个「项目」就是一条 S3 服务配置，支持多项目随时切换。
- **凭据不落 webview**：所有 S3 请求与密钥都只在本机 Rust 侧处理，前端不接触 AK/SK。
- **跨平台、开箱即用**：Windows / Linux / macOS 原生桌面应用，无需命令行基础。

## 功能一览

| # | 功能 | 说明 |
|---|------|------|
| 1 | **项目管理** | 新建 / 编辑 / 删除 S3 服务配置（「项目」），测试连接，切换当前项目；支持限定单桶项目 |
| 2 | **列表查询** | 桶列表与对象列表；虚拟目录树、可点击面包屑、分页加载、前缀过滤、列排序 |
| 3 | **上传** | 文件 / 文件夹上传，保留相对目录结构；支持从系统文件管理器拖拽；大文件自动分片 |
| 4 | **下载** | 单对象或整个「文件夹」（前缀）下载到本地，保留目录结构 |
| 5 | **删除** | 单个 / 多选批量 / 整前缀删除，二次确认，部分失败逐项报告 |
| 6 | **对象预览** | 图片与 UTF-8 文本直接预览；超出大小或二进制文件提示下载查看 |
| 7 | **复制 Key** | 一键复制对象完整 Key，方便贴到代码或 CLI |
| 8 | **任务中心** | 全局浮窗展示进行中 / 已完成 / 失败任务，支持取消、失败重试、清除记录 |

### 亮点细节

- **多项目、可限定单桶**：项目 = 一条 endpoint / region / AK / SK 配置，可选填 `default_bucket`；当账号没有 `ListBuckets` 权限时，填写桶名即可跳过桶列表直接进入。
- **大文件分片上传**：`aws-sdk-s3` 的 `put_object` 不会自动分片，easyS3 对大文件手动走 multipart（分片 ≥ 5MB、流式读取、取消 / 失败自动 `abort`），不把整文件读进内存。
- **传输与项目解耦**：上传 / 下载任务持有自己的客户端快照，任意时刻切换项目都不会中断进行中的任务。
- **中文可读的错误提示**：连接失败、认证失败、权限不足、对象不存在等错误会分类成中文提示，且绝不包含 `secret_key`。
- **凭据安全基线**：配置文件仅当前用户可读（Unix `0600`），错误消息与日志中的密钥统一脱敏。
- **中文与特殊字符文件名**：对象 Key 原样支持中文、空格及 Unicode，本地文件名与 Key 互转不做额外转义。

## 界面与交互

- 左侧为「项目 + 桶」选择区，右侧为对象列表；顶部面包屑显示当前桶 / 目录路径，可点击逐级返回。
- 传输任务中心是全局浮窗，进行中的任务展示实时进度，失败任务提供「重试」。
- 空状态友好：没有项目时显示新建引导页，空桶 / 空目录显示占位提示。
- 支持拖拽上传：从系统文件管理器拖入文件或文件夹即可上传到当前目录。

> 界面截图将在后续版本补充，欢迎在 Issue 中反馈配图。

## 下载与安装

发布包按平台分别构建，可在项目的 GitHub Releases 页面获取（或使用内部制品库分发）：

| 平台 | 安装包格式 |
|------|-----------|
| Windows | NSIS 安装程序（`.exe`） |
| Linux | AppImage、`.deb`（覆盖 x86_64 与 ARM64） |
| macOS | `.dmg` / `.app`（覆盖 Intel 与 Apple Silicon） |

> 注意：**macOS 安装包必须在 macOS 上构建**，Tauri 不支持 macOS 交叉编译。当前不含自动更新，需要手动下载新版本覆盖安装。

## 从源码构建

### 前置条件

- [Node.js](https://nodejs.org/) 与 [pnpm](https://pnpm.io/)
- [Rust stable](https://www.rust-lang.org/tools/install)
- C/C++ 原生编译工具链（Linux 通常为 `build-essential`，Windows 为 MSVC Build Tools，macOS 为 Xcode Command Line Tools）
- 当前平台的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)

Debian/Ubuntu 上的原生工具链通常可通过以下命令安装：

```bash
sudo apt install build-essential
```

### 构建命令

```bash
pnpm install
pnpm tauri dev        # 开发调试
pnpm tauri build      # 打包当前平台
```

仅验证前端可使用 `pnpm build`；核心 Rust 库可独立用 `cargo test -p easys3-core` 验证。

仓库根目录也提供了 `Makefile`，兼容 Windows `cmd.exe` 与 POSIX shell（Git Bash / Linux / macOS）：

```bash
make install          # 安装前端依赖
make tauri-dev        # 启动桌面端开发模式
make verify           # 执行前端构建、格式、Clippy 和测试
make release          # 构建当前平台的 Tauri 发布包
make help             # 查看全部目标
```

> 在 Windows 上构建时，请在「x64 Native Tools Command Prompt for VS」或「Developer PowerShell for VS」中执行，以确保 MSVC 编译链接环境可用；如控制台中文显示乱码，可先执行 `chcp 65001`。

### 指定目标平台

也可以通过 Rust target triple 指定构建目标：

```bash
make core-build TARGET=x86_64-unknown-linux-gnu
make release    TARGET=x86_64-unknown-linux-gnu
```

| Target | 平台 |
| --- | --- |
| `x86_64-unknown-linux-gnu` | Linux x86_64 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |
| `x86_64-pc-windows-msvc` | Windows x86_64（MSVC） |
| `aarch64-pc-windows-msvc` | Windows ARM64（MSVC） |
| `x86_64-apple-darwin` | macOS Intel |
| `aarch64-apple-darwin` | macOS Apple Silicon |

指定目标前需通过 `rustup target add <target>` 安装对应 Rust target，并准备该平台所需的交叉编译器、链接器和 Tauri 系统依赖。跨平台构建不会绕过平台限制，例如 macOS 安装包仍应在 macOS 环境中生成。

### 版本发布

可以通过 `VERSION` 同步更新 `package.json`、`Cargo.toml` 和 `src-tauri/tauri.conf.json` 中的版本号：

```bash
make version VERSION=1.2.3
make release VERSION=1.2.3
```

版本号使用 SemVer 格式，例如 `1.2.3`、`1.2.3-beta.1`。不传 `VERSION` 时，`make release` 使用当前文件中的版本号。

## 使用指南

1. **首次使用**：新建项目（名称、endpoint、AK/SK）→ 测试连接 → 保存 → 进入桶列表（或限定桶）。
2. **日常使用**：选项目 → 桶列表 → 进桶 → 按虚拟目录浏览 → 上传 / 下载 / 删除 / 预览 / 复制 Key。
3. **切换项目**：任意时刻可切换；进行中的上传 / 下载任务不会中断、不会被取消。

## 技术架构

easyS3 采用分层设计，把业务规则与 GUI / IPC 解耦，保证核心逻辑可独立测试：

```text
┌─────────────────────────────┐
│  Vue 3 + TypeScript 前端     │  中文 UI、状态管理、任务中心
│  src/                        │  仅通过 invoke 调用命令，不直接访问 S3
├─────────────────────────────┤
│  Tauri 胶水层                │  命令编排、任务生命周期、进度事件
│  src-tauri/                  │  只做编排，不堆业务逻辑
├─────────────────────────────┤
│  easys3-core 业务核心库      │  配置模型、S3 操作、错误分类
│  crates/easys3-core/         │  无 GUI 依赖，可独立 cargo test / clippy
└─────────────────────────────┘
```

**技术栈**

| 层 | 选型 |
|----|------|
| 桌面框架 | Tauri 2.x |
| 后端 / 核心 | Rust（`aws-sdk-s3`、`tokio`、`serde`、`thiserror`、`uuid`） |
| 前端 | Vue 3（`<script setup>`）+ TypeScript + Vite |
| 包管理 | 前端统一使用 pnpm，Rust 使用 Cargo workspace |
| 目标平台 | Windows、Linux、macOS |

**关键实现约定**

- S3 请求与凭据只放 Rust 侧，经 `#[tauri::command]` 暴露给前端；前端不使用 AWS SDK 或直接 `fetch` S3 endpoint。
- 业务逻辑一律写进 `easys3-core`；`src-tauri` 只做命令编排与任务生命周期管理。
- 任务是自持客户端快照的独立执行单元，通过 `Arc<AtomicBool>` 取消，进度经 `task-update` 事件节流（50ms）推送。
- 项目配置存放于系统应用配置目录的 `projects.json`，由 Rust 侧读写，前端不落 `localStorage`。

## 项目结构

| 目录 | 说明 |
| --- | --- |
| `crates/easys3-core` | 业务核心库：配置模型、S3 操作、错误分类（含单元测试，无 GUI 依赖） |
| `src-tauri` | Tauri 胶水层：命令、传输任务执行器、应用状态、capabilities ACL |
| `src` | Vue 3 + TypeScript 前端（中文 UI）：`api.ts`、`store.ts`、组件目录 |
| `.agents` | 业务规格文档（唯一事实来源：功能范围、配置模型、操作行为与红线） |
| `.github` | Issue / PR 模板 |
| `scripts` | 版本号同步等构建脚本 |

> 业务规则的唯一事实来源在 [`.agents/README.md`](.agents/README.md)；工程与工具链约定见 [`AGENTS.md`](AGENTS.md)。参与业务开发前请先阅读对应规格。

## 开发与验证

常用检查命令（提交前建议全部通过）：

```bash
pnpm build                                                   # 前端类型检查 + 构建
cargo fmt --all -- --check                                   # 格式检查
cargo clippy -p easys3-core --all-targets -- -D warnings     # Clippy
cargo test -p easys3-core                                    # 核心库单元测试
```

或使用聚合目标：

```bash
make verify           # 等价于 frontend-build + fmt-check + clippy + test
```

`src-tauri` 依赖 WebKitGTK / WebView2 等系统库，在缺少这些依赖的环境（如无桌面库的容器）中无法编译；此时仍可独立验证 `easys3-core` 与前端。完整桌面端构建请在 Windows、macOS 或装有 [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/) 的 Linux 上进行。

## 范围与非目标

以下能力**明确不在当前范围内**（如确有需要，欢迎先开 Issue 讨论）：

- 桶的创建 / 删除
- ACL / 权限管理、STS / 临时凭证
- 版本控制（列出 / 恢复历史版本）
- 断点续传（上传下载取消后从头再来）
- 跨项目复制 / 同步 / 移动
- 预签名 URL 分享
- 传输任务持久化（应用重启后任务不恢复）
- 配置导入 / 导出
- 自动更新（手动下载新安装包覆盖安装）

## 参与贡献

欢迎提交 Issue、改进代码或贡献文档。开始之前请阅读：

- [贡献指南](CONTRIBUTING.md) — 开发环境、提交规范与 PR 检查清单
- [行为准则](CODE_OF_CONDUCT.md) — 社区协作的基本约定

请勿在 Issue、日志或提交中包含真实的 S3 endpoint、Access Key、Secret Key、桶名或其他敏感信息。

## 安全

easyS3 将 S3 请求与凭据处理放在 Rust 侧，但仍需妥善保护本机账户、配置文件与所连接服务的权限。报告漏洞前请先阅读 [安全策略](SECURITY.md)，优先通过 GitHub Security Advisories 私下报告，切勿在公开 Issue 中披露未修复的漏洞。

## 许可

easyS3 以 [MIT License](LICENSE) 发布。
