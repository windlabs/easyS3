//! easyS3 业务核心库。
//!
//! 本 crate 承载全部业务规则（见 `.agents/` 规格文档），不依赖 Tauri / GUI，
//! 可独立 `cargo test` / `cargo clippy` 验证。Tauri 胶水层在 `src-tauri`。
//!
//! 规格对应关系：
//! - `config`  <- `.agents/connection-config.md`
//! - `list` / `upload` / `download` / `delete` / `preview` <- `.agents/s3-operations.md`
//! - `error`   <- `.agents/s3-operations.md` 错误分类规范

pub mod config;
pub mod delete;
pub mod download;
pub mod error;
pub mod list;
pub mod plan;
pub mod preview;
pub mod s3;
pub mod upload;
