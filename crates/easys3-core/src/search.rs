//! P1-08 高级对象过滤 DSL：`size>10MB AND modified>30d AND class=GLACIER`。

use crate::error::CoreError;
use serde::Serialize;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Default, Serialize)]
pub struct SearchFilter {
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub modified_within: Option<Duration>,
    pub storage_class: Option<String>,
}

fn parse_size(raw: &str) -> Result<u64, CoreError> {
    let upper = raw.trim().to_ascii_uppercase();
    let (number, unit) = upper
        .trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .trim()
        .parse::<u64>()
        .map_err(|_| CoreError::Other("size 条件格式错误，例如 size>10MB".into()))
        .map(|number| {
            (
                number,
                &upper[upper
                    .find(|c: char| c.is_ascii_alphabetic())
                    .unwrap_or(upper.len())..],
            )
        })?;
    let multiplier = match unit {
        "" | "B" => 1,
        "KB" => 1024,
        "MB" => 1024 * 1024,
        "GB" => 1024 * 1024 * 1024,
        _ => return Err(CoreError::Other("size 单位仅支持 B / KB / MB / GB".into())),
    };
    Ok(number.saturating_mul(multiplier))
}

pub fn parse(input: &str) -> Result<SearchFilter, CoreError> {
    let mut filter = SearchFilter::default();
    if input.trim().is_empty() {
        return Ok(filter);
    }
    for clause in input.split("AND").map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(value) = clause.strip_prefix("size>") {
            filter.min_size = Some(parse_size(value)?);
        } else if let Some(value) = clause.strip_prefix("size<") {
            filter.max_size = Some(parse_size(value)?);
        } else if let Some(value) = clause.strip_prefix("modified>") {
            let days = value
                .trim()
                .strip_suffix('d')
                .ok_or_else(|| CoreError::Other("modified 条件格式错误，例如 modified>30d".into()))?
                .parse::<u64>()
                .map_err(|_| CoreError::Other("modified 天数必须是整数".into()))?;
            filter.modified_within = Some(Duration::from_secs(days.saturating_mul(86400)));
        } else if let Some(value) = clause.strip_prefix("class=") {
            if value.trim().is_empty() {
                return Err(CoreError::Other("class 条件不能为空".into()));
            }
            filter.storage_class = Some(value.trim().to_ascii_uppercase());
        } else {
            return Err(CoreError::Other(format!(
                "不支持的搜索条件“{clause}”；支持 size、modified、class，并用 AND 连接"
            )));
        }
    }
    Ok(filter)
}

pub fn matches(
    filter: &SearchFilter,
    size: u64,
    modified: Option<SystemTime>,
    class: Option<&str>,
) -> bool {
    if filter.min_size.is_some_and(|min| size <= min)
        || filter.max_size.is_some_and(|max| size >= max)
    {
        return false;
    }
    if let Some(within) = filter.modified_within {
        if modified
            .and_then(|time| SystemTime::now().duration_since(time).ok())
            .is_none_or(|age| age > within)
        {
            return false;
        }
    }
    if let Some(expected) = &filter.storage_class {
        if class.unwrap_or("STANDARD").to_ascii_uppercase() != *expected {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_and_matcher() {
        let f = parse("size>10MB AND modified>30d AND class=GLACIER").unwrap();
        assert!(matches(
            &f,
            11 * 1024 * 1024,
            Some(SystemTime::now()),
            Some("GLACIER")
        ));
        assert!(!matches(&f, 1, Some(SystemTime::now()), Some("GLACIER")));
        assert!(parse("bad=x").is_err());
    }
}
