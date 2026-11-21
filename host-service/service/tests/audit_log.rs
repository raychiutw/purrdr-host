//! 稽核紀錄(spec #52、#56、#60、raychiutw/purrdr.host#43):記下配對、連線、撤銷與寫入類操作，不含按鍵、prompt、權杖與配對碼。
//! 紀錄在主機服務資料夾的 `logs/audit.log`,超過上限輪替;舊版放在資料夾根層的 `audit.log` 會搬進 `logs/`。

mod support;

use std::path::Path;
use std::time::SystemTime;

use purrdr_host_service::audit_log::{AuditEvent, AuditLog};
use purrdr_host_service::transcript::TranscriptEnvironment;
use serde_json::{Value, json};
use support::{FakeHerdr, FileServiceOptions, TestClient, http_request, recorded, start_file_service, start_service};

const SECRET_KEYS: &str = "秘密按鍵內容";

fn events(audit_log: &str) -> Vec<Value> {
    audit_log.lines().map(|line| serde_json::from_str(line).expect("每行都是 JSON")).collect()
}

#[tokio::test]
async fn audit_log_records_pairing_connection_writes_and_revocation_without_content() {
    let herdr = FakeHerdr::start(vec![
        ("ping", recorded("ping.ndjson")),
        ("pane.send_keys", json!({"id": "", "result": {"type": "ok"}})),
    ])
    .await;
    let service = start_service(herdr.socket.clone()).await;
    let code = service.pairing_code(false);
    let (_, paired) = http_request(
        &service,
        "POST",
        "/v1/pair",
        None,
        Some(json!({"pairing_code": code, "device_name": "Ray 的 iPhone"})),
    )
    .await;
    let device_id = paired["device_id"].as_str().unwrap().to_owned();
    let device_token = paired["device_token"].as_str().unwrap().to_owned();
    let read_only = service.pair_device("借出的 iPad", true);

    let mut client = TestClient::connect_with_token(&service, Some(&device_token)).await;
    client.request(json!({"type": "hello", "request_id": "r1", "protocol_version": 1})).await;
    client
        .request(json!({"type": "pane.send_keys", "request_id": "r2", "pane_id": "w1:p6", "keys": [SECRET_KEYS]}))
        .await;
    let mut read_only_client = TestClient::connect_with_token(&service, Some(&read_only.device_token)).await;
    read_only_client
        .request(json!({"type": "pane.send_keys", "request_id": "r3", "pane_id": "w1:p6", "keys": [SECRET_KEYS]}))
        .await;
    http_request(&service, "POST", "/v1/revoke", Some(&device_token), None).await;
    let mut revoked_client = TestClient::connect_with_token(&service, Some(&device_token)).await;
    revoked_client.receive().await;

    let audit_log = service.audit_log();
    let events = events(&audit_log);
    let of_device = |id: &str| -> Vec<Value> {
        events
            .iter()
            .filter(|event| event["device_id"] == id)
            .map(|event| {
                let mut summary = event.clone();
                summary.as_object_mut().unwrap().remove("time");
                summary
            })
            .collect()
    };
    assert_eq!(
        of_device(&device_id),
        [
            json!({"event": "paired", "device_id": device_id, "device_name": "Ray 的 iPhone", "read_only": false}),
            json!({"event": "connected", "device_id": device_id}),
            json!({"event": "write", "device_id": device_id, "operation": "pane.send_keys", "pane_id": "w1:p6", "allowed": true}),
            json!({"event": "revoked", "device_id": device_id, "by": "device"}),
            json!({"event": "connection_rejected", "device_id": device_id, "reason": "device_token_revoked"}),
        ]
    );
    assert!(
        of_device(&read_only.device.device_id)
            .contains(&json!({"event": "write", "device_id": read_only.device.device_id, "operation": "pane.send_keys", "pane_id": "w1:p6", "allowed": false}))
    );
    assert!(events.iter().all(|event| event["time"].as_str().is_some_and(|time| time.ends_with('Z'))));
    for secret in [SECRET_KEYS, &device_token, &read_only.device_token, &code, &code.replace('-', "")] {
        assert!(!audit_log.contains(secret), "稽核紀錄含有 {secret}");
    }
}

#[tokio::test]
async fn prompts_and_text_are_audited_by_operation_and_pane_without_content() {
    const SECRET_PROMPT: &str = "秘密 prompt 內容";
    const SECRET_TEXT: &str = "秘密文字內容";
    let herdr = FakeHerdr::start(vec![
        ("ping", recorded("ping.ndjson")),
        ("agent.prompt", json!({"id": "", "result": {"type": "agent_prompted"}})),
        ("pane.send_text", json!({"id": "", "result": {"type": "ok"}})),
    ])
    .await;
    let service = start_service(herdr.socket.clone()).await;
    let mut client = TestClient::connect(&service).await;
    client.request(json!({"type": "hello", "request_id": "r1", "protocol_version": 1})).await;

    client
        .request(json!({"type": "agent.prompt", "request_id": "r2", "pane_id": "w1:p2", "text": SECRET_PROMPT}))
        .await;
    client
        .request(json!({"type": "pane.send_text", "request_id": "r3", "pane_id": "w1:p3", "text": SECRET_TEXT}))
        .await;

    let audit_log = service.audit_log();
    let writes: Vec<(String, String)> = events(&audit_log)
        .into_iter()
        .filter(|event| event["event"] == "write")
        .map(|event| (event["operation"].as_str().unwrap().to_owned(), event["pane_id"].as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(
        writes,
        [
            ("agent.prompt".to_owned(), "w1:p2".to_owned()),
            ("pane.send_text".to_owned(), "w1:p3".to_owned())
        ]
    );
    assert!(!audit_log.contains(SECRET_PROMPT));
    assert!(!audit_log.contains(SECRET_TEXT));
}

// MARK: - 位置、搬移與輪替

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

const OLD_LINE_1: &str = r#"{"time":"2026-11-01T00:00:00Z","event":"pairing_code_created","read_only":false}"#;
const OLD_LINE_2: &str = r#"{"time":"2026-11-01T00:01:00Z","event":"paired","device_id":"d1","device_name":"舊的 iPhone","read_only":false}"#;

/// 以指定的主機服務資料夾啟動(測試先在裡面放東西)。
async fn start_with_state(state: &Path) -> (FakeHerdr, support::TestHostService) {
    let home = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let herdr = FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await;
    let service = start_file_service(
        herdr.socket.clone(),
        FileServiceOptions {
            transcripts: TranscriptEnvironment {
                home: Some(home.keep()),
                ..TranscriptEnvironment::default()
            },
            temporary_directory: None,
            state_directory: Some(state.to_owned()),
        },
    )
    .await;
    (herdr, service)
}

#[tokio::test]
async fn new_records_go_to_logs_audit_log_and_not_the_state_directory_root() {
    let herdr = FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await;
    let service = start_service(herdr.socket.clone()).await;
    let mut client = TestClient::connect(&service).await;
    client.request(json!({"type": "hello", "request_id": "r1", "protocol_version": 1})).await;

    assert!(service.audit_log().contains("\"event\":\"connected\""), "{}", service.audit_log());
    assert!(!service.state_path.join("audit.log").exists());
}

#[tokio::test]
async fn an_audit_log_left_in_the_root_by_an_older_version_is_moved_into_logs_without_losing_records() {
    let state = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    std::fs::write(state.path().join("audit.log"), format!("{OLD_LINE_1}\n{OLD_LINE_2}\n")).unwrap();

    let (_herdr, service) = start_with_state(state.path()).await;
    let mut client = TestClient::connect(&service).await;
    client.request(json!({"type": "hello", "request_id": "r1", "protocol_version": 1})).await;

    assert!(!state.path().join("audit.log").exists(), "根層的舊檔已搬走");
    let moved = read(&state.path().join("logs").join("audit.log"));
    let lines: Vec<&str> = moved.lines().collect();
    assert_eq!(&lines[..2], [OLD_LINE_1, OLD_LINE_2], "舊紀錄在最前面:{moved}");
    assert!(lines[2..].iter().any(|line| line.contains("\"event\":\"connected\"")), "新紀錄接在後面:{moved}");
}

#[tokio::test]
async fn moving_the_old_audit_log_keeps_what_logs_audit_log_already_has() {
    // 升級後、服務重啟前，主機上的配對指令已經往 logs/audit.log 寫了新紀錄。
    let state = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    std::fs::write(state.path().join("audit.log"), format!("{OLD_LINE_1}\n")).unwrap();
    std::fs::create_dir(state.path().join("logs")).unwrap();
    std::fs::write(state.path().join("logs").join("audit.log"), format!("{OLD_LINE_2}\n")).unwrap();

    let (_herdr, _service) = start_with_state(state.path()).await;

    assert!(!state.path().join("audit.log").exists());
    assert_eq!(read(&state.path().join("logs").join("audit.log")), format!("{OLD_LINE_1}\n{OLD_LINE_2}\n"));
}

#[test]
fn the_audit_log_is_rotated_to_dot_one_like_the_service_log() {
    let state = tempfile::tempdir_in(support::test_files_directory()).unwrap();
    let audit = AuditLog::with_maximum_bytes(state.path(), 400);

    for index in 0..12 {
        audit.record(
            AuditEvent::Connected {
                device_id: format!("device-{index:02}"),
            },
            SystemTime::now(),
        );
    }

    let logs = state.path().join("logs");
    let current = read(&logs.join("audit.log"));
    let older = read(&logs.join("audit.log.1"));
    assert!(!older.is_empty(), "超過上限後有舊檔");
    assert!(current.len() <= 400 && older.len() <= 400, "{} / {}", current.len(), older.len());
    assert!(!logs.join("audit.log.2").exists(), "舊檔只留一份");
    // 每一行都完整(輪替發生在行與行之間)，而且新舊檔接起來是連續的最後幾筆。
    let all: Vec<Value> = older
        .lines()
        .chain(current.lines())
        .map(|line| serde_json::from_str(line).expect("整行 JSON"))
        .collect();
    let ids: Vec<&str> = all.iter().map(|event| event["device_id"].as_str().unwrap()).collect();
    assert_eq!(ids.last(), Some(&"device-11"));
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{ids:?}");
}
