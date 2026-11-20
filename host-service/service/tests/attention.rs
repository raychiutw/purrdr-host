//! 卡住與仍在等你的判定(raychiutw/purrdr.host#31):working 時畫面 10 分鐘不變是可能卡住，blocked 持續 15 分鐘是仍在等你;
//! 每個狀態期間只提醒一次。以明確的時間點驅動，不實際等待。

use std::time::{Duration, Instant};

use purrdr_host_service::attention::{Alert, AttentionDetector};

const MINUTE: Duration = Duration::from_secs(60);

fn detector() -> AttentionDetector {
    AttentionDetector::default()
}

#[test]
fn an_unchanged_screen_while_working_is_stuck_after_ten_minutes_once() {
    let start = Instant::now();
    let mut detector = detector();
    detector.status("w1:pA", "working", start);

    assert_eq!(detector.screen("w1:pA", "aaa", start), None);
    assert_eq!(detector.screen("w1:pA", "aaa", start + 9 * MINUTE + Duration::from_secs(59)), None);
    assert_eq!(detector.screen("w1:pA", "aaa", start + 10 * MINUTE), Some(Alert::Stuck));
    assert_eq!(detector.screen("w1:pA", "aaa", start + 30 * MINUTE), None);
}

#[test]
fn a_changing_screen_restarts_the_ten_minutes() {
    let start = Instant::now();
    let mut detector = detector();
    detector.status("w1:pA", "working", start);
    detector.screen("w1:pA", "aaa", start);

    assert_eq!(detector.screen("w1:pA", "bbb", start + 8 * MINUTE), None);
    assert_eq!(detector.screen("w1:pA", "bbb", start + 17 * MINUTE), None);
    assert_eq!(detector.screen("w1:pA", "bbb", start + 18 * MINUTE), Some(Alert::Stuck));
}

#[test]
fn a_new_working_period_can_be_stuck_again_but_other_statuses_never_are() {
    let start = Instant::now();
    let mut detector = detector();
    detector.status("w1:pA", "working", start);
    detector.screen("w1:pA", "aaa", start);
    assert_eq!(detector.screen("w1:pA", "aaa", start + 10 * MINUTE), Some(Alert::Stuck));

    detector.status("w1:pA", "idle", start + 11 * MINUTE);
    assert_eq!(detector.screen("w1:pA", "aaa", start + 11 * MINUTE), None);
    assert_eq!(detector.screen("w1:pA", "aaa", start + 40 * MINUTE), None);

    detector.status("w1:pA", "working", start + 41 * MINUTE);
    detector.screen("w1:pA", "aaa", start + 41 * MINUTE);
    assert_eq!(detector.screen("w1:pA", "aaa", start + 51 * MINUTE), Some(Alert::Stuck));
}

#[test]
fn needing_you_for_fifteen_minutes_reminds_once() {
    let start = Instant::now();
    let mut detector = detector();
    detector.status("w1:pB", "blocked", start);

    assert_eq!(detector.reminders(start + 14 * MINUTE), Vec::<String>::new());
    assert_eq!(detector.reminders(start + 15 * MINUTE), vec!["w1:pB".to_owned()]);
    assert_eq!(detector.reminders(start + 60 * MINUTE), Vec::<String>::new());

    detector.status("w1:pB", "working", start + 61 * MINUTE);
    detector.status("w1:pB", "blocked", start + 62 * MINUTE);
    assert_eq!(detector.reminders(start + 76 * MINUTE), Vec::<String>::new());
    assert_eq!(detector.reminders(start + 77 * MINUTE), vec!["w1:pB".to_owned()]);
}

#[test]
fn the_same_status_reported_again_is_not_a_new_period() {
    let start = Instant::now();
    let mut detector = detector();
    detector.status("w1:pB", "blocked", start);
    detector.status("w1:pB", "blocked", start + 10 * MINUTE);

    assert_eq!(detector.reminders(start + 15 * MINUTE), vec!["w1:pB".to_owned()]);
}

#[test]
fn only_working_panes_need_their_screen_read() {
    let start = Instant::now();
    let mut detector = detector();
    detector.status("w1:pA", "working", start);
    detector.status("w1:pB", "blocked", start);
    detector.status("w1:p6", "idle", start);

    assert_eq!(detector.working_panes(), vec!["w1:pA".to_owned()]);
}
