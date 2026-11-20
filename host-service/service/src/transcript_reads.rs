//! 對話紀錄的列表預覽與 subagent／workflow／委派讀取(raychiutw/purrdr.host#17),規則與 app 的 SSH 腳本相同。

use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

use crate::protocol::{TranscriptPreviewChange, TranscriptWorkflowKind};
use crate::text_format::hex;
use crate::transcript::{Record, TranscriptFailure, strip_images};

/// 列表預覽最多讀的檔尾 bytes(同 SSH 預覽腳本的 `tail -c 48000`)。
const PREVIEW_BYTES: u64 = 48_000;

/// 版本錨點取檔尾前多少 bytes。
const ANCHOR_BYTES: u64 = 64;

/// 一個對話紀錄的預覽。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub change: TranscriptPreviewChange,
    /// `<修改時間>-<大小> <錨點>`,app 原樣帶回。
    pub version: String,
    pub lines: Vec<String>,
}

/// 讀預覽：與 `known_version` 相同回 unchanged;只是附加了不超過 48000 bytes 的完整行時只回附加部分;否則回檔尾。
pub fn preview(path: &Path, known_version: Option<&str>) -> Result<Preview, TranscriptFailure> {
    let mut file = open(path)?;
    let metadata = file.metadata().map_err(|_| TranscriptFailure::Unreadable)?;
    let size = metadata.len();
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs());
    let version = format!("{modified}-{size} {}", anchor(&mut file, size)?);
    if known_version == Some(version.as_str()) {
        return Ok(Preview {
            change: TranscriptPreviewChange::Unchanged,
            version,
            lines: Vec::new(),
        });
    }
    let known_size = match known_version {
        Some(known) => appended_from(&mut file, known, size)?,
        None => None,
    };
    let (change, start) = match known_size {
        Some(known_size) => (TranscriptPreviewChange::Appended, known_size),
        None => (TranscriptPreviewChange::Whole, size.saturating_sub(PREVIEW_BYTES)),
    };
    let content = read_range(&mut file, start, size)?;
    Ok(Preview {
        change,
        version,
        lines: stripped_lines(&content),
    })
}

/// 已知版本是這個檔案較早的樣子(錨點相同、停在行尾)且之後附加不超過 48000 bytes 時，回已知的大小。
fn appended_from(file: &mut File, known_version: &str, size: u64) -> Result<Option<u64>, TranscriptFailure> {
    let Some((stamp, known_anchor)) = known_version.split_once(' ') else {
        return Ok(None);
    };
    let Some(known_size) = stamp.rsplit_once('-').and_then(|(_, size)| size.parse::<u64>().ok()) else {
        return Ok(None);
    };
    if known_size >= size || size - known_size > PREVIEW_BYTES || anchor(file, known_size)? != known_anchor {
        return Ok(None);
    }
    let ends_at_line_end = known_size == 0 || read_range(file, known_size - 1, known_size)? == b"\n";
    Ok(ends_at_line_end.then_some(known_size))
}

/// 位置之前最多 64 bytes 的 SHA-256(前 16 個十六進位字元);位置是 0 時是 `0`。
fn anchor(file: &mut File, end: u64) -> Result<String, TranscriptFailure> {
    if end == 0 {
        return Ok("0".to_owned());
    }
    let bytes = read_range(file, end.saturating_sub(ANCHOR_BYTES), end)?;
    Ok(hex(&Sha256::digest(&bytes)[..8]))
}

/// 依換行切開、剝除圖片;空行略過(同 app 解析 SSH 輸出)。
fn stripped_lines(content: &[u8]) -> Vec<String> {
    content
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| String::from_utf8_lossy(&strip_images(line)).into_owned())
        .collect()
}

/// 對話紀錄的 session 資料夾(放 subagent 與 workflow 檔):主對話紀錄去掉 `.jsonl`;subagent 的對話紀錄取
/// `subagents` 之前的部分(同 app 的 `ClaudeTranscript.sessionFolder`)。
pub fn session_folder(transcript: &Path) -> PathBuf {
    let mut folder = PathBuf::new();
    for component in transcript.components() {
        if component.as_os_str() == SUBAGENTS {
            return folder;
        }
        folder.push(component);
    }
    transcript.with_extension("")
}

const SUBAGENTS: &str = "subagents";

/// session 資料夾的 `subagents/`(subagent 與 workflow agent 的對話紀錄都在這底下)。
pub fn subagents_folder(transcript: &Path) -> PathBuf {
    session_folder(transcript).join(SUBAGENTS)
}

/// 呼叫 id 與 run id 只能含英數、`-`、`_`,1–128 字(同 app 的 `isSafeDelegationID`)。
pub fn is_safe_delegation_id(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// `subagents/` 下 `toolUseId` 完全等於 `call_id` 的 `agent-*.meta.json` 對應的對話紀錄;絕不挑最新檔。
pub fn find_subagent(transcript: &Path, call_id: &str) -> Result<PathBuf, TranscriptFailure> {
    let folder = subagents_folder(transcript);
    if !folder.is_dir() {
        return Err(TranscriptFailure::Absent);
    }
    let entries = std::fs::read_dir(&folder).map_err(|_| TranscriptFailure::Unreadable)?;
    let mut matches = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(agent) = name.strip_prefix("agent-").and_then(|rest| rest.strip_suffix(".meta.json")) else {
            continue;
        };
        let Ok(meta) = std::fs::read(entry.path()) else {
            continue;
        };
        let names_this_call =
            serde_json::from_slice::<serde_json::Value>(&meta).is_ok_and(|meta| meta.get("toolUseId").and_then(serde_json::Value::as_str) == Some(call_id));
        if names_this_call {
            matches.push(folder.join(format!("agent-{agent}.jsonl")));
        }
    }
    match matches.len() {
        0 => Err(TranscriptFailure::Absent),
        1 => Ok(matches.remove(0)),
        _ => Err(TranscriptFailure::Ambiguous),
    }
}

/// workflow 進度：讀到的檔案種類、內容的 checksum,與內容(checksum 與已知的相同時沒有)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowProgress {
    pub kind: TranscriptWorkflowKind,
    pub checksum: String,
    pub content: Option<String>,
}

/// `<session>/workflows/<run>.json`(結束才寫)優先，否則 `<session>/subagents/workflows/<run>/journal.jsonl`。
/// `run_id` 須先以 `is_safe_delegation_id` 檢查。
pub fn workflow(transcript: &Path, run_id: &str, known_checksum: Option<&str>) -> Result<WorkflowProgress, TranscriptFailure> {
    let session = session_folder(transcript);
    let run = session.join("workflows").join(format!("{run_id}.json"));
    let journal = session.join(SUBAGENTS).join("workflows").join(run_id).join("journal.jsonl");
    let (kind, path) = if run.is_file() {
        (TranscriptWorkflowKind::Run, run)
    } else if journal.is_file() {
        (TranscriptWorkflowKind::Journal, journal)
    } else {
        return Err(TranscriptFailure::Absent);
    };
    let bytes = std::fs::read(&path).map_err(|_| TranscriptFailure::Unreadable)?;
    let checksum = checksum(&bytes);
    let content = (known_checksum != Some(checksum.as_str())).then(|| String::from_utf8_lossy(&bytes).into_owned());
    Ok(WorkflowProgress { kind, checksum, content })
}

/// 父對話紀錄中指名委派的行：剝除圖片後內容的 checksum,與紀錄(checksum 與已知的相同時沒有)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationLines {
    pub checksum: String,
    pub records: Option<Vec<Record>>,
}

/// 含任一個 id 的行(同 SSH 的 `grep -F -e <id> -e <id>`,含還沒寫完的最後一行)。
pub fn delegation_lines(transcript: &Path, ids: &[&str], known_checksum: Option<&str>) -> Result<DelegationLines, TranscriptFailure> {
    use std::io::{BufRead, BufReader};
    let mut records = Vec::new();
    let mut hasher = Sha256::new();
    for line in BufReader::new(open(transcript)?).split(b'\n') {
        let line = line.map_err(|_| TranscriptFailure::Unreadable)?;
        if !ids.iter().any(|id| contains(&line, id.as_bytes())) {
            continue;
        }
        let stripped = strip_images(&line);
        hasher.update(&stripped);
        hasher.update(b"\n");
        records.push(Record {
            length: line.len() as u64,
            text: String::from_utf8_lossy(&stripped).into_owned(),
        });
    }
    let checksum = hex(&hasher.finalize());
    let records = (known_checksum != Some(checksum.as_str())).then_some(records);
    Ok(DelegationLines { checksum, records })
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn checksum(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn open(path: &Path) -> Result<File, TranscriptFailure> {
    File::open(path).map_err(|error| match error.kind() {
        ErrorKind::NotFound => TranscriptFailure::Absent,
        _ => TranscriptFailure::Unreadable,
    })
}

fn read_range(file: &mut File, start: u64, end: u64) -> Result<Vec<u8>, TranscriptFailure> {
    file.seek(SeekFrom::Start(start)).map_err(|_| TranscriptFailure::Unreadable)?;
    let mut bytes = Vec::new();
    file.take(end.saturating_sub(start))
        .read_to_end(&mut bytes)
        .map_err(|_| TranscriptFailure::Unreadable)?;
    Ok(bytes)
}
