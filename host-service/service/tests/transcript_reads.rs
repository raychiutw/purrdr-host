//! 對話紀錄的列表預覽與 subagent／workflow／委派讀取(raychiutw/purrdr.host#17):只接受這條連線定位過的對話紀錄，
//! 回傳格式比照 app 的 SSH 腳本。以真正的 WebSocket client 連主機服務，背後是錄製 snapshot 的假 herdr 與暫存家目錄。

mod support;

use std::path::{Path, PathBuf};

use purrdr_host_service::transcript::TranscriptEnvironment;
use serde_json::{Value, json};
use support::{FakeHerdr, TestClient, TestHostService, recorded, start_service_with_transcripts};
use tempfile::TempDir;

/// 錄製 snapshot 裡 `w1:p6` 的 Claude Code session 與它的 cwd 轉成的專案資料夾。
const CLAUDE_SESSION: &str = "00000000-0000-0000-0000-000000000001";
const CLAUDE_PROJECT_FOLDER: &str = "-Users-demo-Projects-garden-journal";

struct Fixture {
    home: TempDir,
    _herdr: FakeHerdr,
    service: TestHostService,
}

impl Fixture {
    async fn start() -> Fixture {
        let home = tempfile::tempdir().unwrap();
        let herdr = FakeHerdr::start(vec![
            ("ping", recorded("ping.ndjson")),
            ("session.snapshot", recorded("session.snapshot.ndjson")),
        ])
        .await;
        let service = start_service_with_transcripts(
            herdr.socket.clone(),
            TranscriptEnvironment {
                home: Some(home.path().to_owned()),
                ..TranscriptEnvironment::default()
            },
        )
        .await;
        Fixture { home, _herdr: herdr, service }
    }

    /// 在暫存家目錄寫一個檔案(含上層資料夾)。逐段 join:Windows 上與主機服務回報的路徑一樣都是 `\`。
    fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = relative.split('/').fold(self.home.path().to_owned(), |path, component| path.join(component));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    /// 錄製 snapshot 裡那個 session 的對話紀錄。
    fn claude_transcript(&self, content: &str) -> PathBuf {
        self.write(&format!(".claude/projects/{CLAUDE_PROJECT_FOLDER}/{CLAUDE_SESSION}.jsonl"), content)
    }

    /// 對話紀錄的 session 資料夾底下的檔案(subagent、workflow)。
    fn session_file(&self, relative: &str, content: &str) -> PathBuf {
        self.write(&format!(".claude/projects/{CLAUDE_PROJECT_FOLDER}/{CLAUDE_SESSION}/{relative}"), content)
    }

    async fn client(&self) -> TestClient {
        let mut client = TestClient::connect(&self.service).await;
        let welcome = client.request(json!({"type": "hello", "request_id": "r0", "protocol_version": 1})).await;
        assert_eq!(welcome["type"], "welcome");
        client
    }

    /// 已握手、已定位錄製 session 對話紀錄的連線。
    async fn located_client(&self) -> TestClient {
        let mut client = self.client().await;
        let located = client
            .request(json!({"type": "transcript.locate", "request_id": "r1", "session": {"kind": "id", "value": CLAUDE_SESSION}, "header": false}))
            .await;
        assert_eq!(located["type"], "transcript.locate.result", "{located}");
        client
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn previews(request_id: &str, entries: Value) -> Value {
    json!({"type": "transcript.previews", "request_id": request_id, "entries": entries})
}

#[tokio::test]
async fn previews_return_the_tail_of_each_located_transcript_with_raw_lengths_and_images_stripped() {
    let fixture = Fixture::start().await;
    let image = "A".repeat(200);
    let content = format!("{{\"n\":1}}\n{{\"image\":\"{image}\"}}\n{{\"n\":3}}\n");
    let transcript = fixture.claude_transcript(&content);
    let mut client = fixture.located_client().await;

    let reply = client.request(previews("r2", json!([{"key": "w1:p6", "path": path_text(&transcript)}]))).await;

    assert_eq!(reply["type"], "transcript.previews.result", "{reply}");
    assert_eq!(reply["request_id"], "r2");
    let preview = &reply["previews"][0];
    assert_eq!(preview["key"], "w1:p6");
    assert_eq!(preview["change"], "whole");
    assert_eq!(
        preview["records"],
        json!([
            {"length": 7, "text": "{\"n\":1}"},
            {"length": 212, "text": "{\"image\":\"\"}"},
            {"length": 7, "text": "{\"n\":3}"}
        ])
    );
    assert!(preview["version"].as_str().is_some_and(|version| !version.is_empty()));
}

#[tokio::test]
async fn previews_refuse_paths_this_connection_did_not_locate() {
    let fixture = Fixture::start().await;
    let transcript = fixture.claude_transcript("{}\n");
    let secret = fixture.write("secret.jsonl", "{\"token\":\"x\"}\n");
    let mut client = fixture.located_client().await;

    let reply = client
        .request(previews(
            "r2",
            json!([{"key": "a", "path": path_text(&transcript)}, {"key": "b", "path": path_text(&secret)}]),
        ))
        .await;

    assert_eq!(reply["code"], "transcript_not_allowed", "{reply}");
    assert_eq!(reply["request_id"], "r2");
}

#[tokio::test]
async fn previews_read_only_the_last_48000_bytes_and_skip_transcripts_that_disappeared() {
    let fixture = Fixture::start().await;
    let filler = format!("{{\"pad\":\"{}\"}}\n", "x".repeat(60_000));
    let transcript = fixture.claude_transcript(&format!("{filler}{{\"n\":2}}\n"));
    let mut client = fixture.located_client().await;
    let located_then_removed = path_text(&transcript);

    let first = client.request(previews("r2", json!([{"key": "a", "path": located_then_removed}]))).await;
    std::fs::remove_file(&transcript).unwrap();
    let after_removal = client.request(previews("r3", json!([{"key": "a", "path": located_then_removed}]))).await;

    let records = first["previews"][0]["records"].as_array().unwrap();
    assert_eq!(records.len(), 2, "{first}");
    let cut_line_length = 48_000 - "{\"n\":2}\n".len() - 1;
    assert_eq!(records[0]["length"], cut_line_length, "第一行從切點算起");
    assert_eq!(records[0]["text"].as_str().unwrap().len(), cut_line_length);
    assert_eq!(records[1], json!({"length": 7, "text": "{\"n\":2}"}));
    assert_eq!(after_removal["previews"], json!([]));
}

fn append(path: &Path, text: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

#[tokio::test]
async fn previews_with_the_known_version_return_unchanged_then_only_appended_lines() {
    let fixture = Fixture::start().await;
    let transcript = fixture.claude_transcript("{\"n\":1}\n");
    let mut client = fixture.located_client().await;
    let path = path_text(&transcript);
    let first = client.request(previews("r2", json!([{"key": "a", "path": path}]))).await;
    let known_version = first["previews"][0]["version"].clone();

    let unchanged = client
        .request(previews("r3", json!([{"key": "a", "path": path, "known_version": known_version}])))
        .await;
    append(&transcript, "{\"n\":2}\n");
    let appended = client
        .request(previews("r4", json!([{"key": "a", "path": path, "known_version": known_version}])))
        .await;

    assert_eq!(unchanged["previews"][0]["change"], "unchanged", "{unchanged}");
    assert_eq!(unchanged["previews"][0]["version"], known_version);
    assert_eq!(unchanged["previews"][0]["records"], json!([]));
    assert_eq!(appended["previews"][0]["change"], "appended", "{appended}");
    assert_eq!(appended["previews"][0]["records"], json!([{"length": 7, "text": "{\"n\":2}"}]));
    assert_ne!(appended["previews"][0]["version"], known_version);
}

#[tokio::test]
async fn previews_read_the_whole_tail_again_when_the_known_version_is_not_an_earlier_state_of_the_file() {
    let fixture = Fixture::start().await;
    let transcript = fixture.claude_transcript("{\"n\":1}\n");
    let mut client = fixture.located_client().await;
    let path = path_text(&transcript);
    let first = client.request(previews("r2", json!([{"key": "a", "path": path}]))).await;
    let known_version = first["previews"][0]["version"].clone();

    std::fs::write(&transcript, "{\"m\":1}\n{\"m\":2}\n").unwrap();
    let replaced = client
        .request(previews("r3", json!([{"key": "a", "path": path, "known_version": known_version}])))
        .await;
    let garbage = client
        .request(previews("r4", json!([{"key": "a", "path": path, "known_version": "not a version"}])))
        .await;

    assert_eq!(replaced["previews"][0]["change"], "whole", "{replaced}");
    assert_eq!(
        replaced["previews"][0]["records"],
        json!([{"length": 7, "text": "{\"m\":1}"}, {"length": 7, "text": "{\"m\":2}"}])
    );
    assert_eq!(garbage["previews"][0]["change"], "whole", "{garbage}");
}

fn subagent(request_id: &str, path: &Path, call_id: &str) -> Value {
    json!({"type": "transcript.subagent", "request_id": request_id, "path": path_text(path), "call_id": call_id})
}

#[tokio::test]
async fn subagent_finds_the_transcript_whose_meta_names_exactly_this_call_and_makes_it_readable() {
    let fixture = Fixture::start().await;
    let transcript = fixture.claude_transcript("{}\n");
    fixture.session_file("subagents/agent-a1.meta.json", "{\"toolUseId\":\"toolu_1\",\n\"agentType\":\"general\"}");
    fixture.session_file("subagents/agent-a2.meta.json", "{\"toolUseId\":\"toolu_12\"}");
    let subagent_transcript = fixture.session_file("subagents/agent-a1.jsonl", "{\"n\":1}\n");
    let mut client = fixture.located_client().await;

    let reply = client.request(subagent("r2", &transcript, "toolu_1")).await;
    let window = client
        .request(json!({"type": "transcript.window", "request_id": "r3", "path": reply["path"], "end": 8, "lines": 10}))
        .await;

    assert_eq!(
        reply,
        json!({"type": "transcript.subagent.result", "request_id": "r2", "path": path_text(&subagent_transcript)})
    );
    assert_eq!(window["records"], json!([{"length": 7, "text": "{\"n\":1}"}]), "{window}");
}

#[tokio::test]
async fn subagent_reports_absent_while_starting_and_refuses_to_guess_between_two_matching_metas() {
    let fixture = Fixture::start().await;
    let transcript = fixture.claude_transcript("{}\n");
    let mut client = fixture.located_client().await;

    let before_folder = client.request(subagent("r2", &transcript, "toolu_1")).await;
    fixture.session_file("subagents/agent-a2.meta.json", "{\"toolUseId\":\"toolu_12\"}");
    let no_exact_match = client.request(subagent("r3", &transcript, "toolu_1")).await;
    fixture.session_file("subagents/agent-a1.meta.json", "{\"toolUseId\":\"toolu_1\"}");
    fixture.session_file("subagents/agent-a3.meta.json", "{\"toolUseId\":\"toolu_1\"}");
    let two_matches = client.request(subagent("r4", &transcript, "toolu_1")).await;

    assert_eq!(before_folder["code"], "transcript_absent", "{before_folder}");
    assert_eq!(no_exact_match["code"], "transcript_absent", "{no_exact_match}");
    assert_eq!(two_matches["code"], "transcript_ambiguous", "{two_matches}");
}

#[tokio::test]
async fn subagent_and_subagent_paths_only_extend_to_the_located_session_folder() {
    let fixture = Fixture::start().await;
    let transcript = fixture.claude_transcript("{}\n");
    let other_session = fixture.write(".claude/projects/-tmp/other.jsonl", "{}\n");
    let other_subagent = fixture.write(".claude/projects/-tmp/other/subagents/agent-x.jsonl", "{}\n");
    fixture.session_file("subagents/notes.txt", "secret\n");
    let session = path_text(&transcript.with_extension(""));
    let escaping = format!("{session}/subagents/../../../-tmp/other.jsonl");
    let mut client = fixture.located_client().await;
    let window = |request_id: &str, path: &str| json!({"type": "transcript.window", "request_id": request_id, "path": path, "end": 3, "lines": 1});

    let replies = [
        client.request(subagent("r2", &other_session, "toolu_1")).await,
        client.request(window("r3", &path_text(&other_subagent))).await,
        client.request(window("r4", &escaping)).await,
        client.request(window("r5", &format!("{session}/subagents/notes.txt"))).await,
    ];

    for reply in replies {
        assert_eq!(reply["code"], "transcript_not_allowed", "{reply}");
    }
}

fn workflow(request_id: &str, path: &Path, run_id: &str, known_checksum: Option<&Value>) -> Value {
    let mut request = json!({"type": "transcript.workflow", "request_id": request_id, "path": path_text(path), "run_id": run_id});
    if let Some(checksum) = known_checksum {
        request["known_checksum"] = checksum.clone();
    }
    request
}

#[tokio::test]
async fn workflow_reads_the_journal_until_the_run_file_is_written_and_skips_content_when_unchanged() {
    let fixture = Fixture::start().await;
    let transcript = fixture.claude_transcript("{}\n");
    let journal = "{\"event\":\"start\"}\n{\"event\":\"agent\"}\n";
    fixture.session_file("subagents/workflows/run_1/journal.jsonl", journal);
    let mut client = fixture.located_client().await;

    let from_journal = client.request(workflow("r2", &transcript, "run_1", None)).await;
    let unchanged = client.request(workflow("r3", &transcript, "run_1", Some(&from_journal["checksum"]))).await;
    fixture.session_file("workflows/run_1.json", "{\"status\":\"completed\"}");
    let from_run = client.request(workflow("r4", &transcript, "run_1", Some(&from_journal["checksum"]))).await;

    assert_eq!(from_journal["type"], "transcript.workflow.result", "{from_journal}");
    assert_eq!(from_journal["kind"], "journal");
    assert_eq!(from_journal["content"], journal);
    assert_eq!(unchanged["kind"], "journal");
    assert_eq!(unchanged["checksum"], from_journal["checksum"]);
    assert_eq!(unchanged.get("content"), None, "{unchanged}");
    assert_eq!(from_run["kind"], "run");
    assert_eq!(from_run["content"], "{\"status\":\"completed\"}");
    assert_ne!(from_run["checksum"], from_journal["checksum"]);
}

#[tokio::test]
async fn workflow_reports_absent_without_either_file_and_rejects_run_ids_that_could_leave_the_session_folder() {
    let fixture = Fixture::start().await;
    let transcript = fixture.claude_transcript("{}\n");
    fixture.write(".claude/projects/secret.json", "{\"token\":\"x\"}");
    let mut client = fixture.located_client().await;

    let absent = client.request(workflow("r2", &transcript, "run_1", None)).await;
    fixture.session_file("workflows/run_2.json", "{}");
    let traversal = client.request(workflow("r3", &transcript, "../../../secret", None)).await;

    assert_eq!(absent["code"], "transcript_absent", "{absent}");
    assert_eq!(traversal["code"], "invalid_message", "{traversal}");
}

#[tokio::test]
async fn delegation_returns_lines_naming_the_call_or_agent_with_raw_lengths_then_only_a_checksum_when_unchanged() {
    let fixture = Fixture::start().await;
    let image = "B".repeat(200);
    let launch = format!("{{\"tool_use_id\":\"toolu_1\",\"image\":\"{image}\"}}");
    let progress = "{\"agentId\":\"agent_9\",\"status\":\"running\"}";
    let transcript = fixture.claude_transcript(&format!("{launch}\n{{\"other\":1}}\n{progress}\n"));
    let mut client = fixture.located_client().await;
    let request = |request_id: &str, known_checksum: Option<&Value>| {
        let mut request = json!({"type": "transcript.delegation", "request_id": request_id, "path": path_text(&transcript),
                                 "call_id": "toolu_1", "agent_id": "agent_9"});
        if let Some(checksum) = known_checksum {
            request["known_checksum"] = checksum.clone();
        }
        request
    };

    let first = client.request(request("r2", None)).await;
    let unchanged = client.request(request("r3", Some(&first["checksum"]))).await;

    assert_eq!(first["type"], "transcript.delegation.result", "{first}");
    assert_eq!(
        first["records"],
        json!([
            {"length": launch.len(), "text": "{\"tool_use_id\":\"toolu_1\",\"image\":\"\"}"},
            {"length": progress.len(), "text": progress}
        ])
    );
    assert_eq!(unchanged["checksum"], first["checksum"]);
    assert_eq!(unchanged.get("records"), None, "{unchanged}");
}

#[tokio::test]
async fn subagent_workflow_and_delegation_reads_refuse_transcripts_this_connection_did_not_locate() {
    let fixture = Fixture::start().await;
    let transcript = fixture.claude_transcript("{\"tool_use_id\":\"toolu_1\"}\n");
    fixture.session_file("subagents/agent-a1.meta.json", "{\"toolUseId\":\"toolu_1\"}");
    fixture.session_file("workflows/run_1.json", "{}");
    let mut client = fixture.client().await;

    let replies = [
        client.request(subagent("r1", &transcript, "toolu_1")).await,
        client.request(workflow("r2", &transcript, "run_1", None)).await,
        client
            .request(json!({"type": "transcript.delegation", "request_id": "r3", "path": path_text(&transcript), "call_id": "toolu_1"}))
            .await,
    ];

    for reply in replies {
        assert_eq!(reply["code"], "transcript_not_allowed", "{reply}");
    }
}
