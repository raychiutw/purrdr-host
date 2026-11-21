//! 上傳圖片與檔案(raychiutw/purrdr.host#16、#41):`file.upload.start`／`file.upload.chunk` 分塊寫到系統暫存目錄底下的
//! `purrdr-uploads*` 資料夾。以真正的 WebSocket client 連上主機服務，家目錄與暫存目錄都是暫存資料夾，
//! 外部行為只看回應、上傳資料夾的檔案與 `audit.log`。

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
    /// 注入給主機服務的系統暫存目錄。
    temp: TempDir,
    _herdr: FakeHerdr,
    service: TestHostService,
}

impl Fixture {
    async fn start() -> Fixture {
        let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
        let temp = tempfile::tempdir_in(support::test_files_directory()).unwrap();
        let herdr = FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await;
        let service = start_file_service(
            herdr.socket.clone(),
            FileServiceOptions {
                transcripts: TranscriptEnvironment {
                    home: Some(home.path().to_owned()),
                    ..TranscriptEnvironment::default()
                },
                temporary_directory: Some(temp.path().to_owned()),
                state_directory: None,
            },
        )
        .await;
        Fixture {
            home,
            temp,
            _herdr: herdr,
            service,
        }
    }

    /// 暫存目錄底下的上傳資料夾(名稱以 `purrdr-uploads` 開頭);還沒建立時是空的。
    fn upload_folders(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with("purrdr-uploads"))
            .collect()
    }

    fn uploads(&self) -> PathBuf {
        let mut folders = self.upload_folders();
        assert_eq!(folders.len(), 1, "暫存目錄底下應該剛好一個上傳資料夾:{folders:?}");
        folders.remove(0)
    }

    /// 上傳資料夾裡的檔名(含暫存檔)。
    fn uploaded_names(&self) -> Vec<String> {
        self.upload_folders()
            .iter()
            .flat_map(|folder| std::fs::read_dir(folder).unwrap())
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
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
async fn chunks_are_stored_under_the_temporary_directory_and_the_absolute_path_is_returned() {
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
    assert!(path.starts_with(fixture.temp.path()), "{path:?} 不在暫存目錄底下");
    assert!(!fixture.home.path().join(".purrdr").exists(), "不再寫進家目錄的 .purrdr");
}

#[tokio::test]
async fn empty_upload_is_stored_after_one_empty_chunk() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    client.request(start("u1", 0, None)).await;
    let reply = client.request(chunk("c1", "u1", 0, b"")).await;

    assert_eq!(reply["received"], 0);
    let path = PathBuf::from(reply["path"].as_str().expect("空檔收到空 chunk 後帶路徑"));
    assert!(std::fs::read(&path).unwrap().is_empty());
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
    let temp = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let uploads = UploadDirectory::under_temporary_directory(temp.path());
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
    let temp = tempfile::tempdir_in(support::test_files_directory()).unwrap();

    UploadDirectory::under_temporary_directory(temp.path())
        .remove_expired(SystemTime::now())
        .unwrap();

    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0, "不為了清理而建資料夾");
}

#[tokio::test(start_paused = true)]
async fn scheduled_cleanup_removes_uploads_once_they_pass_seven_days() {
    let temp = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let uploads = UploadDirectory::under_temporary_directory(temp.path());
    let uploaded_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let old = place_upload(&uploads, "mh1-aaaaaaaa.jpg", uploaded_at);
    let started = tokio::time::Instant::now();
    let clock = move || uploaded_at + 7 * DAY - Duration::from_secs(60) + started.elapsed();

    let _cleanup = spawn_cleanup(vec![uploads.clone()], clock);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(old.exists(), "啟動時還沒滿 7 天");

    tokio::time::sleep(CLEANUP_INTERVAL).await;
    assert!(!old.exists(), "下一次排程時已超過 7 天");
}

#[tokio::test]
async fn host_service_cleans_expired_uploads_when_it_starts() {
    let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let temp = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let uploads = UploadDirectory::under_temporary_directory(temp.path());
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
            temporary_directory: Some(temp.path().to_owned()),
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

// MARK: - 舊的 ~/.purrdr/uploads

#[test]
fn the_old_home_folder_is_cleaned_like_the_new_one_and_removed_once_empty() {
    let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let legacy = UploadDirectory::legacy_under_home(home.path());
    let uploaded_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let old = place_upload(&legacy, "mh1-aaaaaaaa.jpg", uploaded_at);
    let recent = place_upload(&legacy, "mh2-bbbbbbbb.jpg", uploaded_at + 2 * DAY);

    legacy.remove_expired(uploaded_at + 7 * DAY + Duration::from_secs(1)).unwrap();
    assert!(!old.exists());
    assert!(recent.exists());
    assert!(legacy.path().exists(), "還有檔案時資料夾留著");

    legacy.remove_expired(uploaded_at + 9 * DAY + Duration::from_secs(1)).unwrap();
    assert!(!legacy.path().exists(), "清空後移除舊資料夾");
    assert!(home.path().join(".purrdr").exists(), "只移除 uploads,不動 .purrdr");
}

#[test]
fn an_upload_folder_in_the_temporary_directory_is_kept_when_it_becomes_empty() {
    let temp = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let uploads = UploadDirectory::under_temporary_directory(temp.path());
    let uploaded_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    place_upload(&uploads, "mh1-aaaaaaaa.jpg", uploaded_at);

    uploads.remove_expired(uploaded_at + 8 * DAY).unwrap();

    assert!(uploads.path().exists(), "新資料夾不因清空而移除(上傳中的連線還要用)");
}

#[tokio::test]
async fn host_service_also_cleans_the_old_home_folder_when_it_starts() {
    let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let temp = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let legacy = UploadDirectory::legacy_under_home(home.path());
    let old = place_upload(&legacy, "mh1-aaaaaaaa.jpg", SystemTime::now() - 8 * DAY);
    let herdr = FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await;

    let _service = start_file_service(
        herdr.socket.clone(),
        FileServiceOptions {
            transcripts: TranscriptEnvironment {
                home: Some(home.path().to_owned()),
                ..TranscriptEnvironment::default()
            },
            temporary_directory: Some(temp.path().to_owned()),
            state_directory: None,
        },
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while legacy.path().exists() {
        assert!(tokio::time::Instant::now() < deadline, "啟動後沒有清理舊資料夾:{old:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// MARK: - Unix:資料夾的擁有者與權限

/// 目前使用者的 uid:以自己剛建立的檔案的擁有者取得。
#[cfg(unix)]
fn current_uid(directory: &std::path::Path) -> u32 {
    use std::os::unix::fs::MetadataExt;
    tempfile::tempfile_in(directory).unwrap().metadata().unwrap().uid()
}

#[cfg(unix)]
#[tokio::test]
async fn the_upload_folder_is_named_with_the_uid_and_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    client.request(start("u1", 3, None)).await;

    let folder = fixture.uploads();
    let name = folder.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(name, format!("purrdr-uploads-{}", current_uid(fixture.temp.path())));
    assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o700);
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_in_place_of_the_upload_folder_is_refused_and_its_target_untouched() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::start().await;
    // 目標本身是自己的 0700 資料夾:只有「是 symlink」這一點能讓主機服務拒絕。
    let target = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    std::fs::set_permissions(target.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let link = fixture.temp.path().join(format!("purrdr-uploads-{}", current_uid(fixture.temp.path())));
    std::os::unix::fs::symlink(target.path(), &link).unwrap();
    let mut client = fixture.client().await;

    let reply = client.request(start("u1", 3, None)).await;
    let chunked = client.request(chunk("c1", "u1", 0, b"abc")).await;

    assert_eq!(reply["type"], "error", "{reply}");
    assert_eq!(reply["code"], "internal_error");
    assert_eq!(chunked["type"], "error", "沒有開成的上傳不能送分塊");
    assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 0, "symlink 指向的資料夾沒被寫入");
}

#[cfg(unix)]
#[tokio::test]
async fn an_upload_folder_that_others_can_enter_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::start().await;
    let folder = fixture.temp.path().join(format!("purrdr-uploads-{}", current_uid(fixture.temp.path())));
    std::fs::create_dir(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut client = fixture.client().await;

    let reply = client.request(start("u1", 3, None)).await;

    assert_eq!(reply["code"], "internal_error", "{reply}");
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0, "權限過寬的資料夾裡沒有寫入");
    assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o755, "不替別人改權限");
}

#[cfg(unix)]
#[tokio::test]
async fn an_existing_private_upload_folder_is_reused() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::start().await;
    let folder = fixture.temp.path().join(format!("purrdr-uploads-{}", current_uid(fixture.temp.path())));
    std::fs::create_dir(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut client = fixture.client().await;

    client.request(start("u1", 3, None)).await;
    let stored = client.request(chunk("c1", "u1", 0, b"abc")).await;

    assert_eq!(PathBuf::from(stored["path"].as_str().unwrap()).parent(), Some(folder.as_path()));
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
