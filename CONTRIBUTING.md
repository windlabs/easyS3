# 贡献指南

感谢你考虑为 easyS3 做贡献！提交 Issue 或 Pull Request 前，请先确认改动符合项目定位和以下约定。

## 开始之前

1. 先搜索已有的 Issue 和 Pull Request，避免重复提交。
2. 对于较大的功能或会改变用户行为的改动，建议先创建 Issue 讨论方案。
3. 不要提交真实的 S3 endpoint、Access Key、Secret Key、桶名或其他敏感信息。

## 开发环境

- Node.js 与 pnpm
- Rust stable
- C/C++ 原生编译工具链（Linux 通常为 `gcc`/`cc`、`make`，Windows 为 MSVC Build Tools，macOS 为 Xcode Command Line Tools）
- Tauri 2 所需的系统依赖

安装依赖并运行检查：

```bash
pnpm install
pnpm build
cargo fmt --all -- --check
cargo clippy -p easys3-core --all-targets -- -D warnings
cargo test -p easys3-core
```

完整桌面端构建依赖平台环境。在 Linux 上需要安装 Tauri 官方文档列出的 WebKitGTK 等系统依赖；Windows 和 macOS 请分别在对应平台验证打包结果。

## 提交 Pull Request

- 每个 PR 尽量只解决一个问题，并说明用户可见的变化。
- 新增或修改业务行为时，同时补充必要的测试和文档。
- 保持 S3 请求和凭据处理在 Rust 侧，不要在前端引入 AWS SDK 或直接请求 S3。
- PR 描述应包含测试命令及结果；如果无法运行完整桌面端构建，请说明原因。
- 不要提交构建产物、编辑器配置、密钥或本地配置文件。

项目维护者会在审查中关注正确性、安全性、跨平台兼容性和可维护性。

## 版本发布

发布通过 GitHub Actions 自动完成（见 `.github/workflows/release.yml`）：推送 `v*` 标签即触发三平台（Windows NSIS / Linux AppImage + deb / macOS dmg，覆盖 Intel 与 Apple Silicon）矩阵构建，产物汇总为 **Draft Release**，由维护者核对后手动发布。日常发版步骤：

```bash
make version VERSION=1.2.3          # 同步 package.json / Cargo.toml / tauri.conf.json 版本号
git commit -am "chore(release): 1.2.3"
git tag v1.2.3
git push origin main v1.2.3         # 触发 Release 工作流
```

注意事项：

- **tag 必须与应用版本一致**（`v1.2.3` 对应版本 `1.2.3`），不一致时工作流会在校验步骤直接失败。
- 工作流产出的是 Draft Release，请在 GitHub Releases 页面核对资产（5 个安装包 + `SHA256SUMS.txt`）、补充发布说明后再点 Publish。
- 已发布的版本发现问题时，走新的 patch 版本重新发布；**不要删除或复用已发布的 tag**。
- 首次构建较慢（LTO + 单 codegen-units），三平台全量约 20–40 分钟，后续有 Rust 缓存会明显加速。
