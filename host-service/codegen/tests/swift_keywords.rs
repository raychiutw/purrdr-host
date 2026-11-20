//! 欄位或訊息名稱是 Swift 保留字時，產生的 Swift 識別字要加反引號，wire 名稱不變。

use protocol_codegen::Protocol;
use serde_json::json;

fn swift_for(definitions: serde_json::Value) -> String {
    Protocol::parse(&json!({
        "x-protocol-version": 1,
        "x-excluded-file-locations": [],
        "x-temporary-file-locations": [],
        "x-maximum-file-size": 1,
        "x-maximum-file-chunk": 1,
        "$defs": definitions
    }))
    .unwrap()
    .swift()
}

fn assert_contains_all(swift: &str, expected_lines: &[String]) {
    for expected in expected_lines {
        assert!(swift.contains(expected.as_str()), "缺少 {expected}:\n{swift}");
    }
}

#[test]
fn keyword_fields_are_escaped_in_property_init_assignment_and_coding_keys() {
    let keywords = ["extension", "default", "protocol", "self", "init", "in", "true", "operator", "nil", "throws"];
    let properties: serde_json::Map<String, serde_json::Value> = keywords.iter().map(|keyword| (keyword.to_string(), json!({ "type": "string" }))).collect();
    let swift = swift_for(json!({
        "Upload": { "type": "object", "properties": properties, "required": [] }
    }));

    for keyword in keywords {
        let escaped = format!("`{keyword}`");
        assert_contains_all(
            &swift,
            &[
                format!("public var {escaped}: String?\n"),
                format!("{escaped}: String? = nil"),
                format!("self.{escaped} = {escaped}\n"),
                format!("case {escaped}\n"),
            ],
        );
    }
}

#[test]
fn keyword_named_union_variants_are_escaped_in_every_case_reference() {
    let swift = swift_for(json!({
        "Extension": {
            "type": "object",
            "x-write": true,
            "properties": { "type": { "const": "extension.run" } }
        },
        "Ping": {
            "type": "object",
            "properties": { "type": { "const": "ping" } }
        },
        "Message": { "oneOf": [{ "$ref": "#/$defs/Extension" }, { "$ref": "#/$defs/Ping" }] }
    }));

    assert_contains_all(
        &swift,
        &[
            "case `extension`(Extension)\n".to_owned(),
            "case \"extension.run\": self = .`extension`(try Extension(from: decoder))\n".to_owned(),
            "case .`extension`(let message):\n".to_owned(),
            "try container.encode(\"extension.run\", forKey: .type)\n".to_owned(),
            "case .`extension`: return true\n".to_owned(),
        ],
    );
}
