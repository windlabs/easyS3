# 连接（S3 服务配置）规格

## 数据模型

**连接 = 一条 S3 服务配置**。扁平列表，无分组层级；不要引入"分组 / 工作区"之类的上层概念。

```ts
interface ConnectionConfig {
  id: string                 // UUID v4，创建时生成，重命名不变（作为关联键）
  name: string               // 显示名，非空且全局唯一
  endpoint_url: string       // http/https URL，必填
  region: string             // 必填，S3 兼容服务可填任意值，默认 "us-east-1"
  access_key: string         // 必填
  secret_key: string         // 必填
  force_path_style: boolean  // 默认 true（内部以 S3 兼容服务为主）
  default_bucket?: string    // 可选；限定单桶连接（AK 无 ListBuckets 权限时使用），填写后跳过桶列表直接进入
  ca_cert_path?: string      // 可选；服务使用自签证书时指定 CA 证书（PEM 编码）文件路径
  last_test?: { ok: boolean; msg: string; at: number } | null  // 最近一次「测试连接」结果，at 为 Unix 秒
}
```

## 校验规则

- `name` 非空且不与现有连接重名；`endpoint_url` 必须是合法 http/https URL；`access_key` / `secret_key` 非空。
- `endpoint_url` 允许 http（内网 S3 兼容服务常见），表单默认建议 https。
- `default_bucket`（若填写）须符合桶名规范：3–63 位小写字母 / 数字 / 点 / 连字符。
- `ca_cert_path`（若填写）应指向存在的 PEM 编码证书文件，填写后系统证书与该 CA 共同生效；保存时不校验文件（与"保存不强制测试通过"一致），读取失败在构建客户端时以中文错误提示。
- "测试连接"按钮：无 `default_bucket` 时执行一次 ListBuckets；有则对该桶执行 HeadBucket。失败给出分类错误（错误分类见 s3-operations.md）。
- 保存**不强制**测试通过（目标服务可能临时不可用），但 UI 需展示最近一次测试结果，且该结果随连接持久化（`last_test`）；连接相关字段（`endpoint_url` / `region` / `access_key` / `secret_key` / `default_bucket` / `force_path_style` / `ca_cert_path`）任一变更后，持久化结果即作废清空。

## 存储与凭据安全

- 存储文件：`app.path().app_config_dir()/connections.json`，Rust 侧读写；前端仅通过 command 存取，**禁止落 localStorage**。
- 文件结构：`{ "version": 1, "connections": ConnectionConfig[], "current_connection_id": string | null }`；`version` 字段供未来迁移。
- 配置文件损坏（JSON 解析失败）：不得静默清空；将原文件重命名为 `connections.json.bak.<时间戳>` 后以空配置启动，并提示用户。
 - `secret_key` 优先存入 OS Keychain（Windows Credential Manager / macOS Keychain / Linux Secret Service）；无可用 Keychain 时以本机随机主密钥 AES-GCM 加密后落入配置文件。主密钥文件权限限制为 Unix 0600。
 - 前端查询连接状态时，`secret_key` 必须为空；编辑连接时留空表示保留既有密钥。仅新建、显式改密钥、测试连接时可由前端短暂提交明文。
 - UI 中 `secret_key` 用密码框显示（可切换明文预览）；日志、错误消息、前端事件中一律不得出现 `secret_key`（脱敏为 `****`）。
- `ca_cert_path` 为本地路径（非敏感信息），随连接配置明文存储；证书文件内容不落库。
- `last_test.msg` 来自错误分类输出，不得包含 `secret_key`（错误分类规范已保证）。

## P1-09 配置导入 / 导出

- 可导出连接配置文件；默认清空 `secret_key`。用户显式选择“包含凭据”时，必须二次确认并设置导出密码，整个导出内容以密码加密；导入加密文件时必须提供该密码。
- 导入支持“同名跳过”（默认）与“同名覆盖”；格式或版本错误不得修改现有配置。导入的 Secret Key 仍写入 Keychain/AES-GCM 凭据存储，不能直接留在 `connections.json`。
- 可读取 `~/.aws/credentials` 与 `~/.aws/config` 中的 profile，使用 credentials 的 AK/SK 与 config 的 region 生成待导入连接；用户确认后才保存。

## 切换与删除行为

- `current_connection_id` 持久化；重启后自动恢复上次选中的连接。
- 切换 = 重建 S3 客户端并回到桶列表根视图。
- 进行中的传输任务与连接解耦：切换连接不中断、不取消任务。
- 删除连接前二次确认；若删除的是当前连接，自动选中剩余列表的第一个，没有连接时显示新建引导页。
- 支持复制连接：以现有连接为模板新建（预填全部配置，生成新 id，名称默认加"副本"后缀；`last_test` 不复制，需重新测试）。
