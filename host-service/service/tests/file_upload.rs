//! 上傳圖片與檔案(raychiutw/purrdr.host#16):`file.upload.start`／`file.upload.chunk` 分塊寫到家目錄的 `.purrdr/uploads/`。
//! 以真正的 WebSocket client 連上主機服務，家目錄是暫存資料夾，外部行為只看回應、上傳資料夾的檔案與 `audit.log`。

mod support;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use purrdr_host_service::protocol::{MAXIMUM_FILE_CHUNK, MAXIMUM_FILE_SIZE};
use purrdr_host_service::transcript::TranscriptEnvironment;
use purrdr_host_service::uploads::{CLEANUP_INTERVAL, UploadDirectory, spawn_cleanup};
use serde_json::{Value, json};
use support::{FakeHerdr, FileServiceOptions, TestClient, TestHostService, recorded, start_file_service};
use tempfile::TempDir;

struct Fixture {
    home: TempDir,
    _herdr: FakeHerdr,
    service: TestHostService,
}

impl Fixture {
    async fn start() -> Fixture {
        let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
        let herdr = FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await;
        let service = start_file_service(
            herdr.socket.clone(),
            FileServiceOptions {
                transcripts: TranscriptEnvironment {
                    home: Some(home.path().to_owned()),
                    ..TranscriptEnvironment::default()
                },
                temporary_directory: None,
                state_directory: None,
            },
        )
        .await;
        Fixture { home, _herdr: herdr, service }
    }

    fn uploads(&self) -> PathBuf {
        self.home.path().join(".purrdr").join("uploads")
    }

    /// 上傳資料夾裡的檔名(含暫存檔)。
    fn uploaded_names(&self) -> Vec<String> {
        std::fs::read_dir(self.uploads())
            .map(|entries| entries.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }

    async fn client(&self) -> TestClient {
        let mut client = TestClient::connect(&self.service).await;
        client.request(json!({"type": "hello", "request_id": "h", "protocol_version": 1})).await;
        client
    }
}

fn start(request_id: &str, size: usize, extension: Option<&str>) -> Value {
    let mut message = json!({"type": "file.upload.start", "request_id": request_id, "size": size});
    if let Some(extension) = extension {
        message["extension"] = json!(extension);
    }
    message
}

fn chunk(request_id: &str, upload_id: &str, offset: usize, bytes: &[u8]) -> Value {
    json!({"type": "file.upload.chunk", "request_id": request_id, "upload_id": upload_id, "offset": offset, "bytes": STANDARD.encode(bytes)})
}

#[tokio::test]
async fn chunks_are_stored_under_home_uploads_and_the_absolute_path_is_returned() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let started = client.request(start("u1", 11, Some("png"))).await;
    let first = client.request(chunk("c1", "u1", 0, b"hello ")).await;
    let last = client.request(chunk("c2", "u1", 6, b"world")).await;

    assert_eq!(started, json!({"type": "file.upload.start.result", "request_id": "u1", "upload_id": "u1"}));
    assert_eq!(
        first,
        json!({"type": "file.upload.chunk.result", "request_id": "c1", "upload_id": "u1", "received": 6})
    );
    assert_eq!(last["received"], 11);
    let path = PathBuf::from(last["path"].as_str().expect("收滿時帶路徑"));
    assert!(path.is_absolute());
    assert_eq!(path.parent(), Some(fixture.uploads().as_path()));
    assert_eq!(std::fs::read(&path).unwrap(), b"hello world");
    assert_eq!(fixture.uploaded_names().len(), 1, "暫存檔已換成正式檔");
}

#[tokio::test]
async fn upload_larger_than_the_maximum_file_size_is_rejected_before_anything_is_written() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let reply = client.request(start("u1", MAXIMUM_FILE_SIZE as usize + 1, Some("pdf"))).await;

    assert_eq!(reply["type"], "error");
    assert_eq!(reply["request_id"], "u1");
    assert_eq!(reply["code"], "upload_too_large");
    assert!(reply["message"].as_str().unwrap().contains(&MAXIMUM_FILE_SIZE.to_string()), "訊息帶上限");
    assert!(fixture.uploaded_names().is_empty());
}

#[tokio::test]
async fn chunk_larger_than_the_maximum_chunk_is_rejected_and_the_upload_is_discarded() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    let oversized = vec![7u8; MAXIMUM_FILE_CHUNK as usize + 1];
    client.request(start("u1", oversized.len(), None)).await;

    let reply = client.request(chunk("c1", "u1", 0, &oversized)).await;
    let after = client.request(chunk("c2", "u1", 0, b"x")).await;

    assert_eq!(reply["code"], "upload_too_large");
    assert!(reply["message"].as_str().unwrap().contains(&MAXIMUM_FILE_CHUNK.to_string()), "訊息帶上限");
    assert_eq!(after["code"], "invalid_message", "作廢的上傳不能再送分塊");
    assert!(fixture.uploaded_names().is_empty(), "暫存檔已刪除");
}

#[tokio::test]
async fn chunk_past_the_declared_size_is_rejected_and_the_upload_is_discarded() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.request(start("u1", 4, None)).await;

    let reply = client.request(chunk("c1", "u1", 0, b"12345")).await;

    assert_eq!(reply["code"], "invalid_message");
    assert!(fixture.uploaded_names().is_empty());
}

#[tokio::test]
async fn chunk_out_of_order_is_rejected() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.request(start("u1", 8, None)).await;
    client.request(chunk("c1", "u1", 0, b"1234")).await;

    let reply = client.request(chunk("c2", "u1", 2, b"5678")).await;

    assert_eq!(reply["code"], "invalid_message");
    assert!(fixture.uploaded_names().is_empty());
}

#[tokio::test]
async fn read_only_device_cannot_upload() {
    let fixture = Fixture::start().await;
    let read_only = fixture.service.pair_device("借出的 iPad", true);
    let mut client = TestClient::connect_with_token(&fixture.service, Some(&read_only.device_token)).await;
    client.request(json!({"type": "hello", "request_id": "h", "protocol_version": 1})).await;

    let started = client.request(start("u1", 3, None)).await;
    let chunked = client.request(chunk("c1", "u1", 0, b"abc")).await;

    assert_eq!(started["code"], "read_only_device");
    assert_eq!(chunked["code"], "read_only_device");
    assert!(fixture.uploaded_names().is_empty());
}

#[tokio::test]
async fn unfinished_upload_is_deleted_when_the_connection_ends() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.request(start("u1", 8, None)).await;
    client.request(chunk("c1", "u1", 0, b"1234")).await;
    assert_eq!(fixture.uploaded_names().len(), 1, "上傳中的暫存檔");

    drop(client);

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while !fixture.uploaded_names().is_empty() {
        assert!(tokio::time::Instant::now() < deadline, "暫存檔沒有刪掉:{:?}", fixture.uploaded_names());
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

// MARK: - 7 天清理

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// 在上傳資料夾放一個修改時間是 `modified` 的檔案。
fn place_upload(uploads: &UploadDirectory, name: &str, modified: SystemTime) -> PathBuf {
    std::fs::create_dir_all(uploads.path()).unwrap();
    let path = uploads.path().join(name);
    std::fs::write(&path, b"x").unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
    path
}

#[test]
fn files_older_than_seven_days_are_removed_and_newer_ones_kept() {
    let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let uploads = UploadDirectory::under_home(home.path());
    let uploaded_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let old = place_upload(&uploads, "mh1-aaaaaaaa.jpg", uploaded_at);
    let old_partial = place_upload(&uploads, ".upload.abcdef", uploaded_at);
    let recent = place_upload(&uploads, "mh2-bbbbbbbb.jpg", uploaded_at + 2 * DAY);

    uploads.remove_expired(uploaded_at + 7 * DAY - Duration::from_secs(1)).unwrap();
    assert!(old.exists(), "還沒滿 7 天");

    uploads.remove_expired(uploaded_at + 7 * DAY + Duration::from_secs(1)).unwrap();
    assert!(!old.exists());
    assert!(!old_partial.exists(), "中斷留下的暫存檔一併刪除");
    assert!(recent.exists());
}

#[test]
fn removing_expired_uploads_without_an_upload_folder_is_not_an_error() {
    let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();

    UploadDirectory::under_home(home.path()).remove_expired(SystemTime::now()).unwrap();

    assert!(!home.path().join(".purrdr").exists(), "不為了清理而建資料夾");
}

#[tokio::test(start_paused = true)]
async fn scheduled_cleanup_removes_uploads_once_they_pass_seven_days() {
    let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let uploads = UploadDirectory::under_home(home.path());
    let uploaded_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let old = place_upload(&uploads, "mh1-aaaaaaaa.jpg", uploaded_at);
    let started = tokio::time::Instant::now();
    let clock = move || uploaded_at + 7 * DAY - Duration::from_secs(60) + started.elapsed();

    let _cleanup = spawn_cleanup(uploads.clone(), clock);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(old.exists(), "啟動時還沒滿 7 天");

    tokio::time::sleep(CLEANUP_INTERVAL).await;
    assert!(!old.exists(), "下一次排程時已超過 7 天");
}

#[tokio::test]
async fn host_service_cleans_expired_uploads_when_it_starts() {
    let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let uploads = UploadDirectory::under_home(home.path());
    let old = place_upload(&uploads, "mh1-aaaaaaaa.jpg", SystemTime::now() - 8 * DAY);
    let recent = place_upload(&uploads, "mh2-bbbbbbbb.jpg", SystemTime::now() - DAY);
    let herdr = FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await;

    let _service = start_file_service(
        herdr.socket.clone(),
        FileServiceOptions {
            transcripts: TranscriptEnvironment {
                home: Some(home.path().to_owned()),
                ..TranscriptEnvironment::default()
            },
            temporary_directory: None,
            state_directory: None,
        },
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while old.exists() {
        assert!(tokio::time::Instant::now() < deadline, "啟動後沒有清理");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(recent.exists());
}

/// `audit.log` 裡 event 是 upload 的紀錄。
fn upload_records(audit_log: &str) -> Vec<Value> {
    audit_log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|record| record["event"] == "upload")
        .collect()
}

#[tokio::test]
async fn audit_log_records_path_size_and_result_without_contents() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.request(start("u1", 22, Some("txt"))).await;
    client.request(chunk("c1", "u1", 0, b"secret-content-")).await;
    let stored = client.request(chunk("c2", "u1", 15, b"1234567")).await;
    client.request(start("u2", MAXIMUM_FILE_SIZE as usize + 1, None)).await;

    let audit_log = fixture.service.audit_log();
    let records = upload_records(&audit_log);
    assert_eq!(records.len(), 2, "{audit_log}");
    assert_eq!(records[0]["path"], stored["path"]);
    assert_eq!(records[0]["size"], 22);
    assert_eq!(records[0]["result"], "stored");
    assert_eq!(records[1]["size"], MAXIMUM_FILE_SIZE + 1);
    assert_eq!(records[1]["result"], "too_large");
    assert!(!audit_log.contains("secret-content"));
    assert!(!audit_log.contains(&STANDARD.encode(b"secret-content-")));
    assert!(!audit_log.contains("file.upload.chunk"), "分塊不逐則記");
}

/// 主機產生的檔名：`<時間 base36>-<8 個 base36>[.副檔名]`。
fn is_host_generated_name(name: &str, extension: Option<&str>) -> bool {
    let stem = match extension {
        Some(extension) => match name.strip_suffix(&format!(".{extension}")) {
            Some(stem) => stem,
            None => return false,
        },
        None => name,
    };
    let base36 = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase());
    matches!(stem.split_once('-'), Some((time, random)) if base36(time) && random.len() == 8 && base36(random))
}

#[tokio::test]
async fn file_name_is_generated_by_the_host_and_ignores_any_name_from_the_app() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    let mut request = start("u1", 3, Some("PNG"));
    request["name"] = json!("../../.bashrc");
    client.request(request).await;

    let stored = client.request(chunk("c1", "u1", 0, b"abc")).await;

    let path = PathBuf::from(stored["path"].as_str().unwrap());
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(is_host_generated_name(&name, Some("png")), "{name}");
    assert_eq!(path.parent(), Some(fixture.uploads().as_path()));
}

#[tokio::test]
async fn upload_without_extension_gets_a_host_generated_name_without_extension() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    client.request(start("u1", 3, None)).await;

    let stored = client.request(chunk("c1", "u1", 0, b"abc")).await;

    let path = PathBuf::from(stored["path"].as_str().unwrap());
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(is_host_generated_name(&name, None), "{name}");
}

#[tokio::test]
async fn extension_with_other_characters_is_rejected() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    for extension in ["../sh", "tar.gz", "", "abcdefghijklmnopq", "jp g"] {
        let reply = client.request(start("u1", 3, Some(extension))).await;

        assert_eq!(reply["code"], "invalid_message", "{extension:?}");
    }
    assert!(fixture.uploaded_names().is_empty());
}
