//! 主機服務的 log(raychiutw/purrdr.host#42):服務自己寫進主機服務資料夾的 `logs/host-service.log`,
//! 超過上限輪替成 `.1`。不依賴 launchd、systemd 或 Windows 轉接輸出。

mod support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use purrdr_host_service::service_log::RotatingLog;
use purrdr_host_service::transcript::TranscriptEnvironment;
use purrdr_host_service::uploads::UploadDirectory;
use support::{FakeHerdr, FileServiceOptions, recorded, start_file_service};

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn a_log_over_the_limit_is_rotated_to_dot_one_and_only_one_old_file_is_kept() {
    let folder = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let path = folder.path().join("logs").join("test.log");
    let log = RotatingLog::new(&path, 30);

    // 每行 11 bytes(10 個字元加換行):第三行放不下就輪替。
    for line in ["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc", "dddddddddd", "eeeeeeeeee", "ffffffffff"] {
        log.append_line(line).unwrap();
    }

    assert_eq!(read(&path), "eeeeeeeeee\nffffffffff\n");
    assert_eq!(
        read(&folder.path().join("logs").join("test.log.1")),
        "cccccccccc\ndddddddddd\n",
        "舊檔只留最近一份"
    );
    assert!(!folder.path().join("logs").join("test.log.2").exists());
}

#[test]
fn a_single_line_longer_than_the_limit_is_still_written() {
    let folder = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let path = folder.path().join("test.log");
    let log = RotatingLog::new(&path, 5);

    log.append_line("a line much longer than the limit").unwrap();

    assert_eq!(read(&path), "a line much longer than the limit\n");
    assert!(!folder.path().join("test.log.1").exists(), "空檔不輪替");
}

struct Running {
    state: tempfile::TempDir,
    _herdr: FakeHerdr,
    _service: support::TestHostService,
}

impl Running {
    fn log(&self) -> PathBuf {
        self.state.path().join("logs").join("host-service.log")
    }
}

async fn start(temp: &Path) -> Running {
    let state = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let herdr = FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await;
    let service = start_file_service(
        herdr.socket.clone(),
        FileServiceOptions {
            transcripts: TranscriptEnvironment {
                home: Some(home.keep()),
                ..TranscriptEnvironment::default()
            },
            temporary_directory: Some(temp.to_owned()),
            state_directory: Some(state.path().to_owned()),
        },
    )
    .await;
    Running {
        state,
        _herdr: herdr,
        _service: service,
    }
}

async fn wait_for(log: &Path, text: &str) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let content = read(log);
        if content.contains(text) {
            return content;
        }
        assert!(tokio::time::Instant::now() < deadline, "log 裡沒有 {text:?}:{content:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn starting_the_service_writes_a_line_to_logs_host_service_log() {
    let temp = tempfile::tempdir_in(support::test_files_directory()).unwrap();

    let running = start(temp.path()).await;

    let content = wait_for(&running.log(), "listening on http://127.0.0.1:").await;
    assert!(content.contains(env!("CARGO_PKG_VERSION")), "{content}");
}

#[tokio::test]
async fn a_problem_while_running_is_written_to_the_service_log() {
    // 上傳資料夾的位置被一個檔案占住:清理讀不了資料夾，這是服務執行中會遇到的真實錯誤。
    let temp = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let blocked = UploadDirectory::under_temporary_directory(temp.path()).path().to_owned();
    std::fs::write(&blocked, b"not a folder").unwrap();

    let running = start(temp.path()).await;

    let content = wait_for(&running.log(), "cannot clean uploads").await;
    assert!(content.contains(&blocked.display().to_string()), "{content}");
}
