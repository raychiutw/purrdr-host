//! 從 app 列出與撤銷已配對裝置(raychiutw/purrdr.host#19):devices.list 唯讀、devices.revoke 是寫入類操作。

mod support;

use std::time::SystemTime;

use serde_json::{Value, json};
use support::{FakeHerdr, TestClient, TestHostService, recorded, start_service};

fn hello(request_id: &str) -> Value {
    json!({"type": "hello", "request_id": request_id, "protocol_version": 1})
}

async fn herdr() -> FakeHerdr {
    FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await
}

async fn connected(service: &TestHostService, device_token: &str) -> TestClient {
    let mut client = TestClient::connect_with_token(service, Some(device_token)).await;
    client.request(hello("r1")).await;
    client
}

fn listed_device<'a>(reply: &'a Value, device_id: &str) -> Option<&'a Value> {
    reply["devices"].as_array().unwrap().iter().find(|device| device["device_id"] == device_id)
}

#[tokio::test]
async fn devices_list_reports_paired_devices_and_marks_the_requesting_one() {
    let herdr = herdr().await;
    let service = start_service(herdr.socket.clone()).await;
    let tablet = service.pair_device("Ray 的 iPad", true);
    let mut client = connected(&service, &tablet.device_token).await;

    let reply = client.request(json!({"type": "devices.list", "request_id": "r2"})).await;

    assert_eq!(reply["type"], "devices.list.result");
    assert_eq!(reply["request_id"], "r2");
    let listed_tablet = listed_device(&reply, &tablet.device.device_id).expect("列出發出請求的裝置");
    assert_eq!(listed_tablet["name"], "Ray 的 iPad");
    assert_eq!(listed_tablet["read_only"], true);
    assert_eq!(listed_tablet["current"], true);
    assert!(
        listed_tablet["paired_at"].as_str().unwrap().ends_with('Z'),
        "paired_at 是 RFC 3339 UTC:{listed_tablet}"
    );
    assert!(listed_tablet["last_connected_at"].is_string());
    assert_eq!(reply["devices"].as_array().unwrap().len(), 2, "預先配對的裝置也列出:{reply}");
    let others: Vec<&Value> = reply["devices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|device| device["current"] == false)
        .collect();
    assert_eq!(others.len(), 1);
    assert_eq!(others[0]["read_only"], false);
}

#[tokio::test]
async fn devices_list_omits_revoked_devices_and_never_exposes_tokens() {
    let herdr = herdr().await;
    let service = start_service(herdr.socket.clone()).await;
    let lost_phone = service.pair_device("遺失的手機", false);
    service.store().revoke(&lost_phone.device.device_id, SystemTime::now()).unwrap();
    let mut client = connected(&service, &service.device_token).await;

    let reply = client.request(json!({"type": "devices.list", "request_id": "r2"})).await;

    assert!(listed_device(&reply, &lost_phone.device.device_id).is_none(), "已撤銷的裝置不列出:{reply}");
    let reply_text = reply.to_string();
    assert!(!reply_text.contains(&service.device_token));
    assert!(!reply_text.contains("token"), "回應不含權杖或其雜湊:{reply_text}");
}

#[tokio::test]
async fn revoked_device_is_closed_with_4401_on_its_next_message() {
    let herdr = herdr().await;
    let service = start_service(herdr.socket.clone()).await;
    let lost_phone = service.pair_device("遺失的手機", false);
    let mut lost_phone_client = connected(&service, &lost_phone.device_token).await;
    let mut client = connected(&service, &service.device_token).await;

    let reply = client
        .request(json!({"type": "devices.revoke", "request_id": "r2", "device_id": lost_phone.device.device_id}))
        .await;

    assert_eq!(
        reply,
        json!({"type": "devices.revoke.result", "request_id": "r2", "device_id": lost_phone.device.device_id})
    );
    let rejected = lost_phone_client.request(json!({"type": "agents.list", "request_id": "r3"})).await;
    assert_eq!(rejected["code"], "device_token_revoked");
    assert_eq!(lost_phone_client.closed().await, Some((4401, "device_token_revoked".to_owned())));
    let listed = client.request(json!({"type": "devices.list", "request_id": "r4"})).await;
    assert!(listed_device(&listed, &lost_phone.device.device_id).is_none());
}

#[tokio::test]
async fn read_only_device_cannot_revoke_other_devices() {
    let herdr = herdr().await;
    let service = start_service(herdr.socket.clone()).await;
    let tablet = service.pair_device("Ray 的 iPad", true);
    let mut tablet_client = connected(&service, &tablet.device_token).await;
    let target_device_id = service.store().authenticate(&service.device_token).unwrap().device_id;

    let reply = tablet_client
        .request(json!({"type": "devices.revoke", "request_id": "r2", "device_id": target_device_id}))
        .await;

    assert_eq!(reply["code"], "read_only_device");
    assert!(service.store().authenticate(&service.device_token).is_ok(), "被要求撤銷的裝置仍可用");
}

#[tokio::test]
async fn revoking_an_unknown_device_reports_device_unknown() {
    let herdr = herdr().await;
    let service = start_service(herdr.socket.clone()).await;
    let mut client = connected(&service, &service.device_token).await;

    let reply = client
        .request(json!({"type": "devices.revoke", "request_id": "r2", "device_id": "0000000000000000"}))
        .await;

    assert_eq!(reply["type"], "error");
    assert_eq!(reply["request_id"], "r2");
    assert_eq!(reply["code"], "device_unknown");
}

#[tokio::test]
async fn revoking_from_the_app_is_audited_without_tokens() {
    let herdr = herdr().await;
    let service = start_service(herdr.socket.clone()).await;
    let lost_phone = service.pair_device("遺失的手機", false);
    let mut client = connected(&service, &service.device_token).await;
    let requester_id = service.store().authenticate(&service.device_token).unwrap().device_id;

    client
        .request(json!({"type": "devices.revoke", "request_id": "r2", "device_id": lost_phone.device.device_id}))
        .await;

    let audit_log = service.audit_log();
    let revoked_line: Value = audit_log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["event"] == "revoked")
        .unwrap_or_else(|| panic!("沒有撤銷紀錄:{audit_log}"));
    assert_eq!(revoked_line["device_id"], lost_phone.device.device_id.as_str());
    assert_eq!(revoked_line["by"], "app");
    assert_eq!(revoked_line["by_device_id"], requester_id.as_str());
    assert!(!audit_log.contains(&lost_phone.device_token));
    assert!(!audit_log.contains(&service.device_token));
}

#[tokio::test]
async fn listing_or_revoking_devices_before_hello_requires_handshake() {
    let herdr = herdr().await;
    let service = start_service(herdr.socket.clone()).await;
    let lost_phone = service.pair_device("遺失的手機", false);
    let mut client = TestClient::connect_with_token(&service, Some(&service.device_token)).await;

    let listed = client.request(json!({"type": "devices.list", "request_id": "r1"})).await;
    let revoked = client
        .request(json!({"type": "devices.revoke", "request_id": "r2", "device_id": lost_phone.device.device_id}))
        .await;

    assert_eq!(listed["code"], "handshake_required", "{listed}");
    assert_eq!(revoked["code"], "handshake_required", "{revoked}");
    assert!(service.store().authenticate(&lost_phone.device_token).is_ok(), "沒有 hello 就不撤銷");
}
