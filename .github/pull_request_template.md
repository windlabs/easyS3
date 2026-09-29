## 变更说明

<!-- 简要说明此 PR 解决的问题及用户可见的变化。关联 Issue 可写：Fixes #123。 -->

## 类型

- [ ] Bug 修复
- [ ] 新功能
- [ ] 文档或工程改进
- [ ] 重构（无行为变化）

## 验证

- [ ] `pnpm build`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy -p easys3-core --all-targets -- -D warnings`
- [ ] `cargo test -p easys3-core`
- [ ] 已在 PR 描述中说明未执行的检查及原因

## 检查清单

- [ ] 未提交凭据、私有配置、构建产物或真实业务数据
- [ ] S3 请求与凭据仍只在 Rust 侧处理
- [ ] 已补充必要的测试和文档
