# easyS3 版本升级更新方案

> 状态：**方案草案（未执行）**。本文只给路径、范围与步骤，不包含任何代码/构建动作；执行时按"升级流程"逐段推进。
> 配套文档：`.agents/`（业务规格）、`docs/feature-backlog.md`（功能清单）、`.github/workflows/release.yml`（发布流水线）。

---

## 1. 版本现状盘点

| 项 | 现状 | 说明 |
|---|---|---|
| 当前正式版本 | **v1.0.1**（已打 tag `v1.0.1`） | Git 历史：`4bf8e5d feat:easyS3 1.0.1版本` |
| 工作区版本 | **v1.0.2（未提交）** | `package.json` / `Cargo.toml` / `src-tauri/tauri.conf.json` 均已改为 `1.0.2`，但尚未提交、未打 tag |
| 未提交的业务变更 | 任务中心 5s 自动收起、上传/删除确认框入队后即时关闭 | `src/store.ts`、`src/components/TaskCenter.vue`、`src/App.vue` |
| 版本号落点 | 3 处 | `package.json`、`Cargo.toml`（`[workspace.package]`）、`src-tauri/tauri.conf.json`，由 `scripts/set-version.mjs`（`make version`）统一同步 |

当前工作区里 `v1.0.2` 的三处版本号 + 两处交互修复是**一版未发布的增量改动**，应作为本次升级的首个、也是最小发布单元来收尾。

---

## 2. 版本号策略（SemVer）

对齐现有 `make version VERSION=x.y.z` 与 `release.yml` 的 tag 一致性校验，延续 SemVer：

| 类型 | 版本示例 | 使用场景 |
|---|---|---|
| 修补版（patch） | `1.0.2 → 1.0.3` | 修复 bug、小幅交互改进，不改动配置/数据模型，无红线解除 |
| 次版本（minor） | `1.1.0` | 新增功能（从 backlog 落地）、解除既有红线、变更 `config` 数据模型 |
| 主版本（major） | `2.0.0` | 破坏性变更（配置格式不兼容、检测升级的重大行为改变） |

**纪律**：一次发布只打一个 tag；tag 名 `v*` 必须与 `tauri.conf.json` 的 `version` 完全一致（`release.yml` 会自动校验，不一致直接失败）。

---

## 3. 升级范围建议（分三步，由近及远）

### 3.1 第一步：发布 v1.0.2（收尾当前工作区，成本最低）

**范围（已是现状，仅需走完验证 + 提交 + tag + 发布）**：

| 变更 | 位置 | 类型 |
|---|---|---|
| 任务中心自动收起（5s、有进行中/失败项或悬停时不收起） | `src/store.ts`、`TaskCenter.vue` | 交互改进 |
| 上传/删除确认框在任务入队后即时关闭 | `src/App.vue` | 交互修复 |
| 版本号 1.0.1 → 1.0.2 | 三处版本文件 | 版本 |
| 如需：同步更新 `requirements.md` §界面与交互（自动收起规则已存在于规格第 33 行，核对即可） | `.agents/` | 文档 |

**前置检查**：
- `requirements.md` 第 33 行已覆盖"任务中心自动收起"规则，避免规格-代码不一致（可不改）。
- 当前变更未包含业务规格新增项，属 patch 级别，符合 SemVer。

### 3.2 第二步：发布 v1.1.0（首个"文件管理器基线"次版本）

按 `docs/feature-backlog.md` 的"落地建议——迭代一"选型，建议一次集成 **5–7 项** P0（估算 20–30 人日），避免一次装太多：

| 建议纳入 | ID | 优先级理由 | 红线 |
|---|---|---|---|
| 桶内重命名 / 移动 / 复制 | P0-01 | 竞品最普遍的短板，最高频 | 需在 `s3-operations.md` 补"桶内 vs 跨连接"边界 |
| 新建文件夹 | P0-02 | S，高口碑 | 新增 |
| 对象详情面板 | P0-03 | 元数据可见性 | 新增 |
| 存储类别显示与上传选择 | P0-04 | M | 新增 |
| 凭据安全升级（Keychain/加密） | P0-08 | 信任短板 | 偏离明文基线，需同步 `connection-config.md` |
| 深色模式 | P0-11 | 高口碑、低风险 | 新增 |
| 失败自动重试（可并入，也可留到 1.2） | P0-05 | 传输健壮 | 新增 |

> 备选/留到 `1.2.0`：P0-05（自动重试）、P0-06（限速）、P0-07（传输参数）、P0-09（预览扩展 L）、P0-10（下载后打开）。
> 每项落地前**先补 `.agents/` 规格**再写代码（有红线的先与用户确认解除并更新 `requirements.md`）。

### 3.3 第三步及以后：v1.2.0+ 的能力路线

- `1.2.0`：传输健壮组（P0-05 / P0-06 / P0-07 / P0-10），启动 P1-01（断点续传，XL，尽早排期与重评红线）。
- `1.3.0`：P1 效率组（P1-02 预签名 URL、P1-03 版本恢复、P1-08 搜索、P1-10 ZIP、P1-17 只读模式——含红线解除）。
- P2 企业能力按真实用户诉求触发，未明确前不排期。

---

## 4. 升级流程（执行 SOP，本次仅备案不执行）

以 **v1.0.2（或任意后续版本）** 为例，一次发布的标准步骤：

### 4.1 准备阶段
1. **规格先行**：确认本次改动的业务行为已写进 `.agents/` 对应文档；红线变动已在 `requirements.md` 解除。
2. **确定范围**：罗列本次 changelog 条目（功能 / 修复 / 其它），确定版本号类型。

### 4.2 版本号同步
```bash
make version VERSION=1.0.2      # 同步 package.json / Cargo.toml / tauri.conf.json
```

### 4.3 本地验证（本机即可，无需链接桌面库）
```bash
pnpm build                                                  # 前端类型检查 + 构建
cargo fmt --all -- --check                                  # 格式
cargo clippy -p easys3-core --all-targets -- -D warnings    # 核心 Clippy
cargo clippy -p easys3 --all-targets -- -D warnings         # 胶水层 Clippy（不链接，可过）
cargo test -p easys3-core                                   # 核心单测
# 等价聚合：make verify
```
> 环境注意（见 `AGENTS.md`）：本 WSL 无法做 `src-tauri` 链接期构建，完整打包在 CI 完成；本地 `cargo clippy -p easys3` 与 `cargo check` 可过类型/lint。

### 4.4 提交与打 tag
```bash
git add -A
git commit -m "chore: release v1.0.2"
git tag v1.0.2
git push origin main --tags
```

### 4.5 构建发布
- 推送 `v*` tag 触发 `.github/workflows/release.yml`，五目标矩阵构建（Linux x86_64/ARM64 AppImage+deb、Windows x64 NSIS、macOS arm64 + Intel dmg），产出 **Draft Release** + **SHA256SUMS.txt**。
- 人工在 GitHub 核对 Draft（资产齐全、版本号正确）后再发布。

### 4.6 文档与分发收尾
- `README.md` / `README.en.md` 若涉及下载表或文字变更（如新增平台/包格式/版本说明）同步更新。
- 若接入内部制品库分发，按照既有流程推送产物；当前版本**无自动更新**，需提示用户手动覆盖安装（`requirements.md` 红线）。

---

## 5. 依赖升级清单与风险（可选，独立于产品版本）

若"升级更新"目标是**技术栈版本滚动**，按以下原则分池处理：

### 5.1 前端（pnpm，`package.json`）

| 依赖 | 现状（^范围） | 建议动作 | 风险点 |
|---|---|---|---|
| `@tauri-apps/api` / `cli` | `^2.12.0` | 同 minor 内滚动升级 | 与后端 `tauri` 主版本保持一致（均 2.x） |
| `@tauri-apps/plugin-dialog` | `^2.8.0` | 同 minor 滚动 | 权限 ACL 若插件新增能力需补 capabilities |
| `@tauri-apps/plugin-clipboard-manager` | `^2.4.0` | 同 minor 滚动 | 同上 |
| `vue` | `^3.5.x` | 保持 3.x minor 滚动 | 大版本不升（3→4 需整体回归） |
| `vite` | `^8.3.x` | 锁定在当前 major，可选 minor | 大版本升级引入的 transform/plugin 变化需回归 |
| `@vitejs/plugin-vue` | `^6.0.9` | 跟随 vite/ vue 兼容矩阵 | 与 vite 版本强绑定 |
| `typescript` | `^5.9.3` | **固定 5.x，不升 7** | `vue-tsc` 3.x 与 TS 7 不兼容（见 `AGENTS.md` 红线） |
| `vue-tsc` | `^3.3.11` | 仅在与 TS5、vue 兼容矩阵内升 | 升级前核对兼容矩阵 |

### 5.2 Rust（Cargo workspace）

| 依赖 | 现状 | 建议动作 | 风险点 |
|---|---|---|---|
| `tauri` / `tauri-build` | `2.12.0` | minor 滚动，保持 2.x | API 破坏通常在 minor 边界，升后跑 `cargo clippy -p easys3` |
| `aws-sdk-s3` | `1.150.0` | minor 滚动 | 生成 API 变更；回归 5 项操作 + 分片上传 |
| `aws-smithy-runtime-api` / `-http-client` / `-types` | `1.x` / `1.4` / `1.x` | 与 aws-sdk 对齐 | 三者需与 sdk 版本兼容，升级时一起动并核对 |
| `tokio` | `1.53.1` | minor 滚动 | 纯增量 |
| `serde` / `serde_json` / `thiserror` / `uuid` / `base64` | 1 / 1 / 2 / 1 / 0.22 | 视需要滚动 | 低风险 |

### 5.3 升级执行原则
1. **先产品版本、后依赖版本**：依赖升级作为独立提交，不夹带功能改动，便于回滚定位。
2. **minor 滚动**默认允许，**大版本（Vite 8→9、vue 3→4、tauri 2→3、aws-sdk-s3 1→2）单独开评估**，不在发布窗口内做。
3. **升级后统一跑**：`pnpm build` + `cargo fmt/clippy/test` + 关键路径手测（连接、列表、上传大文件分片、下载、删除、预览）。
4. **锁定文件**：`pnpm-lock.yaml`、`Cargo.lock` 一并提交；pnpm 勿混用 npm/yarn。

---

## 6. 回归验证清单（每次发布的必测项）

- 连接：新建 / 编辑 / 删除 / 测试连接 / 多连接切换 / 限定单桶 / 自签 CA / http endpoint。
- 列表：桶列表、虚拟目录、面包屑、分页、前缀过滤、排序、返回桶列表。
- 上传：文件/文件夹、拖拽、大文件分片（≥5MB、取消/失败 abort）、相对目录结构。
- 下载：单对象/前缀、保留目录、冲突策略（覆盖/跳过/取消）。
- 删除：单个/多选/整前缀、二次确认、部分失败逐项报告、失败重试。
- 预览：图片/文本、超限与二进制提示、SVG 不预览。
- 任务中心：进度/速度、取消、重试、清除、自动收起（v1.0.2 新行为）、退出保护。
- 安全：错误消息与日志无 `secret_key`；前端不落 AK/SK。

---

## 7. 建议的决策点（需用户确认后进入执行）

1. **v1.0.2 是否立即发布**：推荐"是"——当前改动已完成且属 patch 级，先固化为稳定版本。
2. **v1.1.0 选型**：是否按 §3.2 建议纳入 P0-01/02/03/04/08/11（+可选 P0-05）。
3. **依赖升级是否纳入本次**：推荐独立于产品版本、以 minor 滚动方式单独处理。
