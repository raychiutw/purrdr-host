//! 背景自動更新的排程(raychiutw/purrdr.host#7):隨機數與 tokio 的暫停時鐘都由測試注入。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use purrdr_host_service::auto_update::{first_check_delay, next_check_delay, run_schedule};
use purrdr_host_service::update::{AutomaticOutcome, UpdateError};

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);

fn up_to_date() -> AutomaticOutcome {
    AutomaticOutcome::UpToDate("0.5.0".parse().unwrap())
}

#[test]
fn first_check_waits_one_to_five_minutes_after_start() {
    assert_eq!(first_check_delay(0.0), MINUTE);
    assert_eq!(first_check_delay(1.0), 5 * MINUTE);
    assert_eq!(first_check_delay(0.5), 3 * MINUTE);
}

#[test]
fn later_checks_are_daily_with_up_to_an_hour_of_jitter() {
    for outcome in [
        up_to_date(),
        AutomaticOutcome::Disabled,
        AutomaticOutcome::Failed(UpdateError::Io("disk full".into())),
    ] {
        assert_eq!(next_check_delay(&outcome, 0.0), 24 * HOUR, "{outcome:?}");
        assert_eq!(next_check_delay(&outcome, 1.0), 25 * HOUR, "{outcome:?}");
    }
}

#[test]
fn network_errors_are_retried_within_the_hour() {
    let offline = AutomaticOutcome::Failed(UpdateError::Network("timeout".into()));

    assert_eq!(next_check_delay(&offline, 0.0), HOUR);
    assert_eq!(next_check_delay(&offline, 1.0), HOUR + 15 * MINUTE);
}

#[tokio::test(start_paused = true)]
async fn schedule_checks_after_the_first_delay_then_daily_and_retries_network_errors_sooner() {
    let started = tokio::time::Instant::now();
    let checked_at = Arc::new(Mutex::new(Vec::new()));
    let outcomes = Arc::new(Mutex::new(vec![
        up_to_date(),
        AutomaticOutcome::Failed(UpdateError::Network("offline".into())),
        up_to_date(),
        up_to_date(),
    ]));
    let (recorded, remaining) = (checked_at.clone(), outcomes.clone());

    let schedule = tokio::spawn(run_schedule(
        || 0.0,
        move || {
            recorded.lock().unwrap().push(started.elapsed());
            let outcome = remaining.lock().unwrap().remove(0);
            async move { outcome }
        },
    ));
    let four_checks = async {
        while checked_at.lock().unwrap().len() < 4 && !schedule.is_finished() {
            tokio::time::sleep(MINUTE).await;
        }
    };
    let waited = tokio::time::timeout(72 * HOUR, four_checks).await;
    assert!(!schedule.is_finished(), "排程不應該結束：{:?}", schedule.await);
    schedule.abort();
    assert!(waited.is_ok(), "72 小時內沒有檢查 4 次：{:?}", checked_at.lock().unwrap());

    let checked_at = checked_at.lock().unwrap().clone();
    assert_eq!(
        checked_at,
        [MINUTE, MINUTE + 24 * HOUR, MINUTE + 25 * HOUR, MINUTE + 49 * HOUR],
        "第一次 1 分鐘後;之後每 24 小時;網路錯誤 1 小時後重試"
    );
}
