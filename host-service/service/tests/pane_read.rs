//! 讀 Pane 畫面(raychiutw/purrdr.host#28):轉送 herdr pane.read,回傳文字與畫面指紋，不解析語意。

mod support;

use serde_json::{Value, json};
use support::{FakeHerdr, TestClient, recorded, start_service};

fn hello(request_id: &str) -> Value {
    json!({"type": "hello", "request_id": request_id, "protocol_version": 1})
}

fn recorded_text(name: &str) -> String {
    recorded(name)["result"]["read"]["text"].as_str().unwrap().to_owned()
}

async fn herdr_replying(reply: Value) -> FakeHerdr {
    FakeHerdr::start(vec![("ping", recorded("ping.ndjson")), ("pane.read", reply)]).await
}

fn forwarded_read(herdr: &FakeHerdr) -> Value {
    herdr.requests().into_iter().find(|request| request["method"] == "pane.read").expect("沒有送出 pane.read")["params"].clone()
}

#[tokio::test]
async fn visible_text_returns_the_screen_and_its_fingerprint() {
    let herdr = herdr_replying(recorded("pane.read.question.ndjson")).await;
    let service = start_service(herdr.socket.clone()).await;
    let mut client = TestClient::connect(&service).await;
    client.request(hello("r1")).await;

    let reply = client.request(json!({"type": "pane.read", "request_id": "r2", "pane_id": "w9:p1", "source": "visible"})).await;

    assert_eq!(
        reply,
        json!({
            "type": "pane.read.result",
            "request_id": "r2",
            "text": recorded_text("pane.read.question.ndjson"),
            // 以 Python hashlib 對錄製檔文字另外算的 SHA-256。
            "screen": "5f829abb029288a3079406db769fc99a6002ff9505cbeea295b177ee49ba992f",
            "truncated": false,
        })
    );
    assert_eq!(forwarded_read(&herdr), json!({"pane_id": "w9:p1", "source": "visible", "format": "text"}));
}

#[tokio::test]
async fn ansi_keeps_escape_sequences_and_asks_herdr_not_to_strip_them() {
    let herdr = herdr_replying(recorded("pane.read.ansi.question.ndjson")).await;
    let service = start_service(herdr.socket.clone()).await;
    let mut client = TestClient::connect(&service).await;
    client.request(hello("r1")).await;

    let reply = client
        .request(json!({"type": "pane.read", "request_id": "r2", "pane_id": "w9:p1", "source": "recent", "format": "ansi", "lines": 80}))
        .await;

    assert_eq!(reply["screen"], "373e71c18baf82f2446cc0cb07feb8c338004916c033b0de4304d4a6c273d31a");
    assert!(reply["text"].as_str().unwrap().contains('\u{1b}'));
    assert_eq!(
        forwarded_read(&herdr),
        json!({"pane_id": "w9:p1", "source": "recent", "format": "ansi", "strip_ansi": false, "lines": 80})
    );
}

#[tokio::test]
async fn pane_ids_that_are_not_pane_terminal_targets_never_reach_herdr() {
    for pane_id in ["", "-w1:p1", "w1:p1;rm", "w1 p1"] {
        let herdr = herdr_replying(recorded("pane.read.question.ndjson")).await;
        let service = start_service(herdr.socket.clone()).await;
        let mut client = TestClient::connect(&service).await;
        client.request(hello("r1")).await;

        let reply = client.request(json!({"type": "pane.read", "request_id": "r2", "pane_id": pane_id, "source": "visible"})).await;

        assert_eq!(reply["code"], "invalid_message", "{pane_id:?}");
        assert_eq!(herdr.methods(), ["ping"], "{pane_id:?}");
    }
}

#[tokio::test]
async fn unknown_panes_report_the_herdr_error() {
    let herdr = herdr_replying(recorded("error.pane-not-found.ndjson")).await;
    let service = start_service(herdr.socket.clone()).await;
    let mut client = TestClient::connect(&service).await;
    client.request(hello("r1")).await;

    let reply = client.request(json!({"type": "pane.read", "request_id": "r2", "pane_id": "w9:pZ", "source": "visible"})).await;

    assert_eq!(reply["type"], "error");
    assert_eq!(reply["code"], "herdr_error");
    assert_eq!(reply["herdr_code"], "pane_not_found");
}

#[tokio::test]
async fn read_only_devices_can_read_the_screen() {
    let herdr = herdr_replying(recorded("pane.read.plan.ndjson")).await;
    let service = start_service(herdr.socket.clone()).await;
    let device = service.pair_device("iPad", true);
    let mut client = TestClient::connect_with_token(&service, Some(&device.device_token)).await;
    client.request(hello("r1")).await;

    let reply = client.request(json!({"type": "pane.read", "request_id": "r2", "pane_id": "w9:p1", "source": "visible"})).await;

    assert_eq!(reply["screen"], "8a58485a53c41e264556371d2d22a46a0343c51f1dd54fe3fc72971bf5cda52f");
}
