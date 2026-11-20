//! 唯讀 git 狀態與 diff(raychiutw/purrdr.host#33):cwd 必須是 herdr 回報的 Agent 工作目錄，以程序內函式庫讀取。
//! 暫存 repo 也以函式庫(gix)建立，不執行 git 指令。

mod support;

use std::path::Path;

use gix::bstr::BStr;
use gix::objs::tree::{Entry, EntryKind};
use serde_json::{Value, json};
use support::{FakeHerdr, TestClient, TestHostService, recorded, start_service};
use tempfile::TempDir;

fn hello(request_id: &str) -> Value {
    json!({"type": "hello", "request_id": request_id, "protocol_version": 1})
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

const README_BEFORE: &str = "# Garden\n\nWater daily.\n";
const README_AFTER: &str = "# Garden\n\nWater twice daily.\n";
const LOGO: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

/// HEAD:.env、.gitignore、README.md、logo.png、notes.txt;index 另外加了 TODO.md(已 add 未 commit)。
/// 工作目錄:.env、README.md、logo.png 改了，notes.txt 刪了，src/garden.rs 是新檔，target/ 被 .gitignore 排除。
fn garden_repository() -> TempDir {
    let directory = TempDir::new().unwrap();
    let repo = gix::init(directory.path()).unwrap();
    // 預設分支不受主機的 init.defaultBranch 影響。
    std::fs::write(directory.path().join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
    let blob = |bytes: &[u8]| repo.write_blob(bytes).unwrap().detach();
    let head_files: Vec<(&str, &[u8])> = vec![
        (".env", b"TOKEN=old\n"),
        (".gitignore", b"target/\n"),
        ("README.md", README_BEFORE.as_bytes()),
        ("logo.png", LOGO),
        ("notes.txt", b"keep\n"),
    ];
    let entries = |files: &[(&str, &[u8])]| {
        let mut entries: Vec<Entry> = files
            .iter()
            .map(|(name, bytes)| Entry {
                mode: EntryKind::Blob.into(),
                filename: (*name).into(),
                oid: blob(bytes),
            })
            .collect();
        entries.sort_by(|left, right| left.filename.cmp(&right.filename));
        gix::objs::Tree { entries }
    };
    let head_tree = repo.write_object(entries(&head_files)).unwrap().detach();
    let signature = gix::actor::SignatureRef {
        name: BStr::new("demo"),
        email: BStr::new("demo@example.com"),
        time: "1763600000 +0000",
    };
    repo.commit_as(signature, signature, "HEAD", "garden journal", head_tree, None::<gix::ObjectId>)
        .unwrap();
    let mut index_files = head_files.clone();
    index_files.push(("TODO.md", b"- water the basil\n"));
    let index_tree = repo.write_object(entries(&index_files)).unwrap().detach();
    repo.index_from_tree(&index_tree).unwrap().write(Default::default()).unwrap();

    let root = directory.path();
    let write = |relative: &str, bytes: &[u8]| {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    };
    write(".env", b"TOKEN=new\n");
    write(".gitignore", b"target/\n");
    write("README.md", README_AFTER.as_bytes());
    write("logo.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR changed");
    write("TODO.md", b"- water the basil\n");
    write("src/garden.rs", b"fn main() {}\n");
    write("target/out.bin", b"ignored");
    directory
}

/// 錄製的 snapshot,把第一個 Agent 的工作目錄換成 `cwd`。
fn snapshot_with_agent_in(cwd: &Path) -> Value {
    let mut snapshot = recorded("session.snapshot.ndjson");
    snapshot["result"]["snapshot"]["agents"][0]["cwd"] = json!(path_text(cwd));
    snapshot
}

async fn connected(cwd_reported_by_herdr: &Path) -> (FakeHerdr, TestHostService, TestClient) {
    let herdr = FakeHerdr::start(vec![
        ("ping", recorded("ping.ndjson")),
        ("session.snapshot", snapshot_with_agent_in(cwd_reported_by_herdr)),
    ])
    .await;
    let service = start_service(herdr.socket.clone()).await;
    let mut client = TestClient::connect(&service).await;
    client.request(hello("r1")).await;
    (herdr, service, client)
}

#[tokio::test]
async fn status_lists_changes_against_head_and_skips_ignored_files() {
    let repository = garden_repository();
    let (_herdr, _service, mut client) = connected(repository.path()).await;

    let reply = client
        .request(json!({"type": "git.status", "request_id": "r2", "cwd": path_text(repository.path())}))
        .await;

    assert_eq!(
        reply,
        json!({
            "type": "git.status.result",
            "request_id": "r2",
            "branch": "main",
            "files": [
                {"path": ".env", "change": "modified"},
                {"path": "README.md", "change": "modified"},
                {"path": "TODO.md", "change": "added"},
                {"path": "logo.png", "change": "modified"},
                {"path": "notes.txt", "change": "deleted"},
                {"path": "src/garden.rs", "change": "untracked"},
            ],
        })
    );
}

#[tokio::test]
async fn diff_shows_text_hunks_and_withholds_binary_and_excluded_files() {
    let repository = garden_repository();
    let (_herdr, _service, mut client) = connected(repository.path()).await;

    let reply = client
        .request(json!({"type": "git.diff", "request_id": "r2", "cwd": path_text(repository.path())}))
        .await;

    assert_eq!(reply["truncated"], false);
    let files = reply["files"].as_array().unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|file| file["path"] == path)
            .unwrap_or_else(|| panic!("沒有 {path}:{reply}"))
            .clone()
    };
    // git diff 的 hunk 格式(前後 3 行)。
    assert_eq!(
        file("README.md"),
        json!({"path": "README.md", "change": "modified", "diff": "@@ -1,3 +1,3 @@\n # Garden\n \n-Water daily.\n+Water twice daily.\n"})
    );
    assert_eq!(file("logo.png"), json!({"path": "logo.png", "change": "modified", "omitted": "binary"}));
    assert_eq!(file(".env"), json!({"path": ".env", "change": "modified", "omitted": "excluded"}));
    assert!(file("notes.txt")["diff"].as_str().unwrap().contains("-keep\n"));
    assert!(file("src/garden.rs")["diff"].as_str().unwrap().contains("+fn main() {}\n"));
    assert_eq!(files.len(), 6);
}

#[tokio::test]
async fn diff_of_one_path_returns_only_that_file() {
    let repository = garden_repository();
    let (_herdr, _service, mut client) = connected(repository.path()).await;

    let reply = client
        .request(json!({"type": "git.diff", "request_id": "r2", "cwd": path_text(repository.path()), "path": "TODO.md"}))
        .await;
    let unchanged = client
        .request(json!({"type": "git.diff", "request_id": "r3", "cwd": path_text(repository.path()), "path": ".gitignore"}))
        .await;

    assert_eq!(
        reply["files"],
        json!([{"path": "TODO.md", "change": "added", "diff": "@@ -0,0 +1 @@\n+- water the basil\n"}])
    );
    assert_eq!(unchanged["files"], json!([]));
}

#[tokio::test]
async fn files_over_the_size_limit_are_not_diffed() {
    let repository = garden_repository();
    let mut large = "water\n".repeat(26 * 1024 * 1024 / 6 + 1).into_bytes();
    large.truncate(26 * 1024 * 1024);
    std::fs::write(repository.path().join("src").join("huge.log"), large).unwrap();
    let (_herdr, _service, mut client) = connected(repository.path()).await;

    let reply = client
        .request(json!({"type": "git.diff", "request_id": "r2", "cwd": path_text(repository.path()), "path": "src/huge.log"}))
        .await;

    assert_eq!(reply["files"], json!([{"path": "src/huge.log", "change": "untracked", "omitted": "too_large"}]));
}

#[tokio::test]
async fn only_working_directories_reported_by_herdr_are_read() {
    let repository = garden_repository();
    let elsewhere = TempDir::new().unwrap();
    let (_herdr, _service, mut client) = connected(elsewhere.path()).await;

    for request in [
        json!({"type": "git.status", "request_id": "r2", "cwd": path_text(repository.path())}),
        json!({"type": "git.diff", "request_id": "r2", "cwd": path_text(repository.path())}),
    ] {
        let reply = client.request(request.clone()).await;
        assert_eq!(reply["code"], "cwd_not_allowed", "{request}");
    }
}

#[tokio::test]
async fn a_working_directory_outside_a_repository_says_so() {
    let plain = TempDir::new().unwrap();
    let (_herdr, _service, mut client) = connected(plain.path()).await;

    let reply = client
        .request(json!({"type": "git.status", "request_id": "r2", "cwd": path_text(plain.path())}))
        .await;

    assert_eq!(reply["code"], "not_git_repository");
}

#[tokio::test]
async fn a_working_directory_inside_the_repository_only_sees_its_own_changes() {
    let repository = garden_repository();
    let src = repository.path().join("src");
    let (_herdr, _service, mut client) = connected(&src).await;

    let status = client.request(json!({"type": "git.status", "request_id": "r2", "cwd": path_text(&src)})).await;
    let diff = client.request(json!({"type": "git.diff", "request_id": "r3", "cwd": path_text(&src)})).await;
    let outside = client
        .request(json!({"type": "git.diff", "request_id": "r4", "cwd": path_text(&src), "path": "README.md"}))
        .await;

    assert_eq!(status["files"], json!([{"path": "src/garden.rs", "change": "untracked"}]));
    assert_eq!(diff["files"].as_array().unwrap().len(), 1);
    assert_eq!(outside["files"], json!([]));
}
