//! 送出前比對(raychiutw/purrdr.host#29):agent.prompt、pane.send_text、pane.send_keys 帶 expected_session／expected_screen 時，
//! 先比對 herdr 目前的 session 與畫面，不符就不送並回 pane_changed;沒帶時照舊直接送。

mod support;

use serde_json::{Value, json};
use support::{FakeHerdr, TestClient, recorded, start_service};

fn hello(request_id: &str) -> Value {
    json!({"type": "hello", "request_id": request_id, "protocol_version": 1})
}

/// session.snapshot.ndjson 裡 w1:p6 的 Agent session(錄製時換成的假 UUID)。
const P6_SESSION: &str = "00000000-0000-0000-0000-000000000001";
/// pane.read.question.ndjson 文字的 SHA-256(以 Python hashlib 另外算)。
const QUESTION_SCREEN: &str = "5f829abb029288a3079406db769fc99a6002ff9505cbeea295b177ee49ba992f";

async fn herdr() -> FakeHerdr {
    FakeHerdr::start(vec![
        ("ping", recorded("ping.ndjson")),
        ("session.snapshot", recorded("session.snapshot.ndjson")),
        ("pane.read", recorded("pane.read.question.ndjson")),
        ("agent.prompt", recorded("agent.prompt.ndjson")),
        ("pane.send_text", json!({"id": "", "result": {"type": "ok"}})),
        ("pane.send_keys", recorded("pane.send_keys.ndjson")),
    ])
    .await
}

async fn send(request: Value) -> (Value, Vec<String>) {
    let herdr = herdr().await;
    let service = start_service(herdr.socket.clone()).await;
    let mut client = TestClient::connect(&service).await;
    client.request(hello("r1")).await;
    let reply = client.request(request).await;
    (reply, herdr.methods())
}

#[tokio::test]
async fn matching_session_is_sent() {
    let (reply, methods) =
        send(json!({"type": "agent.prompt", "request_id": "r2", "pane_id": "w1:p6", "text": "next step", "expected_session": P6_SESSION})).await;

    assert_eq!(reply["type"], "agent.prompt.result", "{reply}");
    assert_eq!(methods, ["ping", "session.snapshot", "agent.prompt"]);
}

#[tokio::test]
async fn a_different_session_is_not_sent() {
    let other_session = "00000000-0000-0000-0000-000000000002";
    for request in [
        json!({"type": "agent.prompt", "request_id": "r2", "pane_id": "w1:p6", "text": "next step", "expected_session": other_session}),
        json!({"type": "pane.send_text", "request_id": "r2", "pane_id": "w1:p6", "text": "yes", "expected_session": other_session}),
        json!({"type": "pane.send_keys", "request_id": "r2", "pane_id": "w1:p6", "keys": ["enter"], "expected_session": other_session}),
    ] {
        let (reply, methods) = send(request.clone()).await;

        assert_eq!(reply["type"], "error", "{request}");
        assert_eq!(reply["code"], "pane_changed", "{request}");
        assert_eq!(reply["request_id"], "r2");
        assert_eq!(methods, ["ping", "session.snapshot"], "{request}");
    }
}

#[tokio::test]
async fn a_pane_without_an_agent_is_a_changed_pane() {
    let (reply, methods) =
        send(json!({"type": "pane.send_keys", "request_id": "r2", "pane_id": "w9:p1", "keys": ["enter"], "expected_session": P6_SESSION})).await;

    assert_eq!(reply["code"], "pane_changed");
    assert_eq!(methods, ["ping", "session.snapshot"]);
}

#[tokio::test]
async fn without_expectations_nothing_is_checked() {
    let (reply, methods) = send(json!({"type": "pane.send_keys", "request_id": "r2", "pane_id": "w1:p6", "keys": ["enter"]})).await;

    assert_eq!(reply, json!({"type": "pane.send_keys.result", "request_id": "r2"}));
    assert_eq!(methods, ["ping", "pane.send_keys"]);
}

#[tokio::test]
async fn the_same_screen_is_sent() {
    let (reply, methods) = send(json!({
        "type": "pane.send_keys", "request_id": "r2", "pane_id": "w1:p6", "keys": ["2"],
        "expected_session": P6_SESSION, "expected_screen": QUESTION_SCREEN,
    }))
    .await;

    assert_eq!(reply["type"], "pane.send_keys.result", "{reply}");
    assert_eq!(methods, ["ping", "session.snapshot", "pane.read", "pane.send_keys"]);
}

#[tokio::test]
async fn a_different_screen_is_not_sent() {
    let (reply, methods) = send(json!({
        "type": "pane.send_text", "request_id": "r2", "pane_id": "w1:p6", "text": "2",
        "expected_screen": "0000000000000000000000000000000000000000000000000000000000000000",
    }))
    .await;

    assert_eq!(reply["code"], "pane_changed");
    assert_eq!(methods, ["ping", "pane.read"]);
}
