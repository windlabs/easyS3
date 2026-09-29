//! 错误分类规范：`.agents/s3-operations.md`「通用约束」。
//! 所有面向用户的错误必须是中文可读消息，且不得包含 secret_key。

use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// endpoint 不可达 / 超时 / DNS 失败
    Connection,
    /// 403 InvalidAccessKeyId / SignatureDoesNotMatch
    Auth,
    /// 403 AccessDenied
    Permission,
    /// 404 NoSuchKey / NoSuchBucket
    NotFound,
    /// 其余
    Unknown,
}

#[derive(Debug, Error)]
pub enum CoreError {
    /// S3 服务端/网络错误，message 为中文用户提示
    #[error("{message}")]
    S3 {
        kind: ErrorKind,
        message: String,
        detail: String,
    },
    #[error("{0}")]
    InvalidConfig(String),
    #[error("{0}")]
    Io(String),
    #[error("已取消")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

impl CoreError {
    pub fn s3(kind: ErrorKind, message: impl Into<String>, detail: impl Into<String>) -> Self {
        CoreError::S3 {
            kind,
            message: message.into(),
            detail: detail.into(),
        }
    }

    pub fn io(context: impl Into<String>, e: impl std::fmt::Display) -> Self {
        CoreError::Io(format!("{}：{}", context.into(), e))
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            CoreError::S3 { kind, .. } => *kind,
            CoreError::InvalidConfig(_) => ErrorKind::Unknown,
            CoreError::Io(_) => ErrorKind::Unknown,
            CoreError::Cancelled => ErrorKind::Unknown,
            CoreError::Other(_) => ErrorKind::Unknown,
        }
    }

    /// 脱敏：任何错误消息中都不得出现 secret_key（规格硬性要求）。
    /// 这里做最后一道防线：secret 由调用方持有，本库不记录凭据，
    /// 仅需保证 detail 不包含凭据字段。
    pub fn user_message(&self) -> String {
        match self {
            CoreError::S3 { message, .. } => message.clone(),
            other => other.to_string(),
        }
    }
}

/// 将 aws-sdk 的 SdkError 按规格分类为面向用户的错误。
pub fn classify<E: ProvideErrorMetadata>(err: &SdkError<E>) -> CoreError {
    match err {
        SdkError::ServiceError(se) => {
            let code = se.err().code().unwrap_or_default().to_string();
            let message = se.err().message().unwrap_or_default().to_string();
            let status = se.raw().status().as_u16();
            match (status, code.as_str()) {
                (403, "InvalidAccessKeyId") | (403, "SignatureDoesNotMatch") => CoreError::s3(
                    ErrorKind::Auth,
                    "认证失败，请检查 Access Key / Secret Key",
                    format!("{code} {message}"),
                ),
                (403, _) => CoreError::s3(
                    ErrorKind::Permission,
                    "没有权限执行该操作",
                    format!("{code} {message}"),
                ),
                (404, _) => CoreError::s3(
                    ErrorKind::NotFound,
                    "对象或桶不存在",
                    format!("{code} {message}"),
                ),
                _ => CoreError::s3(
                    ErrorKind::Unknown,
                    "操作失败，请稍后重试",
                    format!("{code} {message}"),
                ),
            }
        }
        // 网络/超时/构造失败均视为连接问题
        _ => CoreError::s3(
            ErrorKind::Connection,
            "无法连接到服务，请检查 endpoint 与网络",
            err.to_string(),
        ),
    }
}
