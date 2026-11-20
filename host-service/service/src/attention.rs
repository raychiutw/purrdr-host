//! 卡住與仍在等你的判定(raychiutw/purrdr.host#31)。
//!
//! 每個 pane 一個「狀態期間」(狀態真的改變時開始):working 期間畫面指紋連續 `stuck_after` 不變是可能卡住，
//! blocked 持續 `reminder_after` 是仍在等你。每個期間最多提醒一次。時間由呼叫端傳入，判定本身不讀時鐘。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// 畫面不變多久算可能卡住。
pub const STUCK_AFTER: Duration = Duration::from_secs(10 * 60);
/// 停在「需要你」多久再提醒。
pub const REMINDER_AFTER: Duration = Duration::from_secs(15 * 60);

/// working 期間的提醒(仍在等你由 `reminders` 回報)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Stuck,
}

#[derive(Debug)]
struct Period {
    status: String,
    since: Instant,
    /// 最近一次的畫面指紋與它從何時開始沒變。
    screen: Option<(String, Instant)>,
    alerted: bool,
}

/// 各 pane 的狀態期間。
#[derive(Debug)]
pub struct AttentionDetector {
    pub stuck_after: Duration,
    pub reminder_after: Duration,
    periods: BTreeMap<String, Period>,
}

impl Default for AttentionDetector {
    fn default() -> Self {
        AttentionDetector::new(STUCK_AFTER, REMINDER_AFTER)
    }
}

impl AttentionDetector {
    pub fn new(stuck_after: Duration, reminder_after: Duration) -> Self {
        AttentionDetector {
            stuck_after,
            reminder_after,
            periods: BTreeMap::new(),
        }
    }

    /// herdr 回報的狀態;跟目前一樣時不是新期間。
    pub fn status(&mut self, pane_id: &str, status: &str, now: Instant) {
        if self.periods.get(pane_id).is_some_and(|period| period.status == status) {
            return;
        }
        self.periods.insert(
            pane_id.to_owned(),
            Period {
                status: status.to_owned(),
                since: now,
                screen: None,
                alerted: false,
            },
        );
    }

    /// working 的 pane 讀到的畫面指紋;這次判定為可能卡住時回 `Stuck`。
    pub fn screen(&mut self, pane_id: &str, fingerprint: &str, now: Instant) -> Option<Alert> {
        let period = self.periods.get_mut(pane_id).filter(|period| period.status == "working")?;
        match &period.screen {
            Some((previous, _)) if previous == fingerprint => {}
            _ => {
                period.screen = Some((fingerprint.to_owned(), now));
                return None;
            }
        }
        let unchanged_since = period.screen.as_ref().map(|(_, since)| *since)?;
        if period.alerted || now.saturating_duration_since(unchanged_since) < self.stuck_after {
            return None;
        }
        period.alerted = true;
        Some(Alert::Stuck)
    }

    /// 這次判定為仍在等你的 pane。
    pub fn reminders(&mut self, now: Instant) -> Vec<String> {
        let mut due = Vec::new();
        for (pane_id, period) in &mut self.periods {
            if period.status == "blocked" && !period.alerted && now.saturating_duration_since(period.since) >= self.reminder_after {
                period.alerted = true;
                due.push(pane_id.clone());
            }
        }
        due
    }

    /// 需要讀畫面的 pane(working 且這期間還沒提醒過)。
    pub fn working_panes(&self) -> Vec<String> {
        self.periods
            .iter()
            .filter(|(_, period)| period.status == "working" && !period.alerted)
            .map(|(pane_id, _)| pane_id.clone())
            .collect()
    }
}
