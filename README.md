# easyS3

基于 [Tauri 2](https://tauri.app/) 的开源 Amazon S3 协议桌面客户端：对象存储的**列表查询、上传、下载、删除、预览**，中文界面，支持 Windows / Linux / macOS。

- **项目 = 一条 S3 服务配置**（endpoint / region / AK / SK，可选限定单桶），支持多项目配置与随时切换
- 大文件上传自动 multipart 分片；传输在后台任务中心进行，支持取消与失败重试
- 对象可直接预览（图片 / UTF-8 文本）；文件名支持中文与空格
- 凭据只存本机配置文件（0600），S3 请求全部在 Rust 侧发起，不进 webview

## 开发

需要安装 Node.js、pnpm、Rust stable、C/C++ 原生编译工具链，以及当前平台的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

在 Debian/Ubuntu Linux 上，原生工具链通常可通过以下命令安装：

```bash
sudo apt install build-essential
```

```bash
pnpm install
pnpm tauri dev        # 开发调试
pnpm tauri build      # 打包
```

仅验证前端可使用 `pnpm build`；核心 Rust 库可独立使用 `cargo test -p easys3-core` 验证。
项目也提供了根目录 `Makefile`，常用目标如下：

```bash
make install          # 安装依赖
make tauri-dev        # 启动桌面端开发模式
make verify           # 执行前端构建、格式、Clippy 和测试
make release          # 构建当前平台的 Tauri 发布包
make help             # 查看全部目标
```

`make release` 会调用 Tauri CLI 的当前平台构建流程。Windows、Linux 和 macOS 发布包应分别在对应平台构建；macOS 包必须在 macOS 上生成。

Makefile 兼容 Windows `cmd.exe` 与 POSIX shell（Git Bash / Linux / macOS）。在 Windows 上构建时，请在「x64 Native Tools Command Prompt for VS」或「Developer PowerShell for VS」中执行，以确保 MSVC 编译链接环境可用；如控制台中文显示乱码，可先执行 `chcp 65001`。

也可以通过 Rust target triple 指定构建目标：

```bash
make core-build TARGET=x86_64-unknown-linux-gnu
make release TARGET=x86_64-unknown-linux-gnu
```

常见目标示例：

| Target | 平台 |
| --- | --- |
| `x86_64-unknown-linux-gnu` | Linux x86_64 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |
| `x86_64-pc-windows-msvc` | Windows x86_64（MSVC） |
| `aarch64-pc-windows-msvc` | Windows ARM64（MSVC） |
| `x86_64-apple-darwin` | macOS Intel |
| `aarch64-apple-darwin` | macOS Apple Silicon |

指定目标前需要通过 `rustup target add <target>` 安装对应 Rust target，并准备该平台所需的交叉编译器、链接器和 Tauri 系统依赖。跨平台构建并不会绕过平台限制；例如 macOS 安装包仍应在 macOS 环境中生成。

发布时可以通过 `VERSION` 同步更新 `package.json`、`Cargo.toml` 和 `src-tauri/tauri.conf.json` 中的版本号：

```bash
make version VERSION=1.2.3
make release VERSION=1.2.3
```

版本号使用 SemVer 格式，例如 `1.2.3`、`1.2.3-beta.1`。不传 `VERSION` 时，`make release` 使用当前文件中的版本号。

完整的开发、贡献和安全报告说明见：

- [贡献指南](CONTRIBUTING.md)
- [安全策略](SECURITY.md)
- [行为准则](CODE_OF_CONDUCT.md)

## 结构

| 目录 | 说明 |
| --- | --- |
| `crates/easys3-core` | 业务核心库：配置模型、S3 操作、错误分类（含单元测试，无 GUI 依赖） |
| `src-tauri` | Tauri 胶水层：命令、传输任务执行器、应用状态 |
| `src` | Vue 3 + TypeScript 前端（中文 UI） |

## 开源许可

easyS3 以 [MIT License](LICENSE) 发布。欢迎提交 Issue、改进代码或贡献文档。
