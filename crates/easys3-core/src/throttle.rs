//! 全局带宽限速：跨任务共享的令牌桶，速率可在任务进行中热调整。
//! 规格：`.agents/requirements.md`「传输设置」。
//!
//! 语义：`acquire(bytes)` 预扣发送配额，欠账时休眠等待；
//! 正余额上限为 100ms 配额（无整秒突发），保证实测速率不超过设定值约 ±10%。
//! rate = 0 表示不限速，`acquire` 立即返回（吞吐与改造前一致）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 正余额容量：100ms 配额（限速精度来源）
const BURST_SECS: f64 = 0.1;

pub struct Throttle {
    /// 字节 / 秒；0 = 不限速。Atomic 支持运行中热调整（保存设置即时生效）。
    rate_bps: AtomicU64,
    state: Mutex<BucketState>,
}

struct BucketState {
    /// 可正可负：负值 = 欠账（上次 acquire 预支的配额）
    tokens: f64,
    last: Instant,
}

impl Throttle {
    pub fn new(rate_bps: u64) -> Self {
        Throttle {
            rate_bps: AtomicU64::new(rate_bps),
            state: Mutex::new(BucketState {
                tokens: 0.0,
                last: Instant::now(),
            }),
        }
    }

    pub fn set_rate_bps(&self, rate_bps: u64) {
        self.rate_bps.store(rate_bps, Ordering::Relaxed);
    }

    pub fn rate_bps(&self) -> u64 {
        self.rate_bps.load(Ordering::Relaxed)
    }

    /// 预扣 bytes 的发送配额；不足则休眠等待（等待中速率调整会被下一轮感知）。
    pub async fn acquire(&self, bytes: u64) {
        while let Some(wait) = self.plan(bytes) {
            tokio::time::sleep(wait).await;
        }
    }

    /// 计算本次 acquire 需要的等待时间；返回 None = 配额已扣，可立即发送。
    fn plan(&self, bytes: u64) -> Option<Duration> {
        let rate = self.rate_bps();
        if rate == 0 || bytes == 0 {
            return None;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        state.tokens = refill(state.tokens, rate, now.duration_since(state.last));
        state.last = now;
        state.tokens -= bytes as f64;
        if state.tokens >= 0.0 {
            return None;
        }
        // 欠账 / 速率 = 等待时间；下限 1ms 防止舍入导致的忙等
        let secs = (-state.tokens) / rate as f64;
        Some(Duration::from_secs_f64(secs).max(Duration::from_millis(1)))
    }
}

/// 纯函数：按流逝时间补充令牌。正余额封顶在 100ms 配额；欠账（负值）全额保留，
/// 由后续等待期间的流逝时间偿还——这是多任务共享下不超发配额的关键。
fn refill(tokens: f64, rate_bps: u64, elapsed: Duration) -> f64 {
    let filled = tokens + elapsed.as_secs_f64() * rate_bps as f64;
    let capacity = rate_bps as f64 * BURST_SECS;
    filled.min(capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refill_caps_positive_balance_at_burst() {
        // 长时间空闲只积累 100ms 配额，不允许整秒突发
        let t = refill(0.0, 1_000_000, Duration::from_secs(60));
        assert_eq!(t, 100_000.0);
    }

    #[test]
    fn refill_preserves_debt_for_repayment() {
        // 欠账 8MB、速率 1MB/s、流逝 8s：欠账恰好还清（不留正余额）
        let t = refill(-8_000_000.0, 1_000_000, Duration::from_secs(8));
        assert_eq!(t, 0.0);
    }

    #[test]
    fn refill_partial_repayment_keeps_remaining_debt() {
        let t = refill(-8_000_000.0, 1_000_000, Duration::from_secs(3));
        assert_eq!(t, -5_000_000.0);
    }

    #[test]
    fn unlimited_rate_never_waits() {
        let throttle = Throttle::new(0);
        assert_eq!(throttle.rate_bps(), 0);
        assert!(throttle.plan(1024 * 1024).is_none());
    }

    #[test]
    fn plan_charges_first_acquire_as_debt() {
        let throttle = Throttle::new(1_000_000);
        let wait = throttle.plan(8_000_000).unwrap();
        // 构造到首次 plan 之间的流逝会补充极少量令牌，等待时间略小于 8s
        assert!(
            wait > Duration::from_secs(7) && wait <= Duration::from_secs(8),
            "unexpected wait: {wait:?}"
        );
    }

    #[test]
    fn rate_is_hot_adjustable() {
        let throttle = Throttle::new(1024);
        assert_eq!(throttle.rate_bps(), 1024);
        throttle.set_rate_bps(0);
        assert_eq!(throttle.rate_bps(), 0);
    }
}
