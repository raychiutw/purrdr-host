//! 唯讀 git 狀態與 diff(raychiutw/purrdr.host#33)。
//!
//! 以程序內函式庫(gix)讀 HEAD、index 與工作目錄，比較出相對 HEAD 的變更;不執行 git,也不套用 filter
//! (gix 的 status／blob-diff 會帶入 filter 程式執行，所以自己比對：stat 與 index 相同就沿用 index 的 id,否則雜湊工作目錄的內容)。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gix::ObjectId;
use gix::bstr::{BString, ByteSlice};
use gix_imara_diff::{Algorithm, BasicLineDiffPrinter, Diff, InternedInput, Token, UnifiedDiffConfig, UnifiedDiffPrinter};

use crate::host_files;
use crate::protocol::{GitChangeKind, GitDiffOmitted, GitFileChange, GitFileDiff, MAXIMUM_FILE_SIZE};

/// untracked 檔案最多走訪多少個項目(避免巨大的工作目錄拖住服務)。
const MAXIMUM_WALKED_ENTRIES: usize = 50_000;

/// 讀不了 repo 的原因。
#[derive(Debug)]
pub(crate) enum GitFailure {
    /// cwd 不在 git repo 的工作目錄裡。
    NotRepository,
    /// 讀取失敗(物件、index 或檔案)。
    Read(String),
}

/// 相對 HEAD 的變更清單與目前分支。
pub(crate) struct Changes {
    pub branch: Option<String>,
    pub files: Vec<GitFileChange>,
}

/// 一個變更與它 diff 需要的兩邊。
struct Change {
    path: String,
    change: GitChangeKind,
    head: Option<ObjectId>,
}

struct Repository {
    repo: gix::Repository,
    workdir: PathBuf,
}

/// 變更清單。
pub(crate) fn status(cwd: &Path) -> Result<Changes, GitFailure> {
    let repository = open(cwd)?;
    let branch = repository.repo.head_name().ok().flatten().map(|name| name.shorten().to_string());
    let files = changes(&repository)?
        .into_iter()
        .map(|change| GitFileChange {
            path: change.path,
            change: change.change,
        })
        .collect();
    Ok(Changes { branch, files })
}

/// 每個變更檔案的 unified diff;`only` 只回這個路徑。回傳 (檔案, 是否因總量上限截斷)。
pub(crate) fn diff(cwd: &Path, only: Option<&str>) -> Result<(Vec<GitFileDiff>, bool), GitFailure> {
    let repository = open(cwd)?;
    let mut budget = MAXIMUM_FILE_SIZE;
    let mut truncated = false;
    let mut files = Vec::new();
    for change in changes(&repository)?.into_iter().filter(|change| only.is_none_or(|path| path == change.path)) {
        let mut file = GitFileDiff {
            path: change.path.clone(),
            change: change.change,
            diff: None,
            omitted: None,
        };
        match file_diff(&repository, &change)? {
            Ok(text) if text.len() as u64 <= budget => {
                budget -= text.len() as u64;
                file.diff = Some(text);
            }
            Ok(_) => {
                truncated = true;
                file.omitted = Some(GitDiffOmitted::TooLarge);
            }
            Err(omitted) => file.omitted = Some(omitted),
        }
        files.push(file);
    }
    Ok((files, truncated))
}

fn open(cwd: &Path) -> Result<Repository, GitFailure> {
    let repo = gix::discover(cwd).map_err(|_| GitFailure::NotRepository)?;
    let workdir = repo.workdir().ok_or(GitFailure::NotRepository)?.to_owned();
    Ok(Repository { repo, workdir })
}

fn read_failure(error: impl std::fmt::Display) -> GitFailure {
    GitFailure::Read(error.to_string())
}

fn changes(repository: &Repository) -> Result<Vec<Change>, GitFailure> {
    let repo = &repository.repo;
    let head = head_blobs(repo)?;
    let index = repo.index_or_empty().map_err(read_failure)?;
    let mut indexed: BTreeMap<BString, &gix::index::Entry> = BTreeMap::new();
    for entry in index.entries() {
        if entry.stage_raw() == 0 && !entry.mode.is_submodule() {
            indexed.insert(entry.path(&index).to_owned(), entry);
        }
    }
    let tracked: BTreeSet<&BString> = head.keys().chain(indexed.keys()).collect();
    let mut changes = Vec::new();
    for path in &tracked {
        let head_id = head.get(*path).copied();
        let full = repository.workdir.join(gix::path::from_bstr(path.as_bstr()).map_err(read_failure)?);
        let worktree_id = worktree_id(repo, &full, indexed.get(*path).copied())?;
        let change = match (head_id, worktree_id) {
            (Some(_), None) => GitChangeKind::Deleted,
            (None, Some(_)) => GitChangeKind::Added,
            (Some(before), Some(after)) if before != after => GitChangeKind::Modified,
            _ => continue,
        };
        changes.push(Change {
            path: path.to_string(),
            change,
            head: head_id,
        });
    }
    for path in untracked(repository, &index, &tracked)? {
        changes.push(Change {
            path,
            change: GitChangeKind::Untracked,
            head: None,
        });
    }
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(changes)
}

/// HEAD tree 底下所有檔案(含 symlink,不含 submodule)的 blob id;還沒有 commit 時是空的。
fn head_blobs(repo: &gix::Repository) -> Result<BTreeMap<BString, ObjectId>, GitFailure> {
    let mut blobs = BTreeMap::new();
    let tree = repo.head_tree_id_or_empty().map_err(read_failure)?.detach();
    let mut pending = vec![(BString::default(), tree)];
    while let Some((prefix, tree_id)) = pending.pop() {
        let tree = repo.find_tree(tree_id).map_err(read_failure)?;
        for entry in tree.iter() {
            let entry = entry.map_err(read_failure)?;
            let mut path = prefix.clone();
            if !path.is_empty() {
                path.push(b'/');
            }
            path.extend_from_slice(entry.filename());
            let mode = entry.mode();
            if mode.is_tree() {
                pending.push((path, entry.object_id()));
            } else if !mode.is_commit() {
                blobs.insert(path, entry.object_id());
            }
        }
    }
    Ok(blobs)
}

/// 工作目錄這個路徑的 blob id;不在或是資料夾時 None。stat 與 index 相同時沿用 index 的 id。
fn worktree_id(repo: &gix::Repository, full: &Path, indexed: Option<&gix::index::Entry>) -> Result<Option<ObjectId>, GitFailure> {
    let Ok(metadata) = std::fs::symlink_metadata(full) else {
        return Ok(None);
    };
    if metadata.is_dir() {
        return Ok(None);
    }
    if let Some(entry) = indexed {
        let unchanged = gix::index::fs::Metadata::from_path_no_follow(full)
            .ok()
            .and_then(|current| gix::index::entry::Stat::from_fs(&current).ok())
            .is_some_and(|current| entry.stat.matches(&current, Default::default()) && entry.stat.size != 0);
        if unchanged {
            return Ok(Some(entry.id));
        }
    }
    let bytes = worktree_bytes(full, &metadata).map_err(read_failure)?;
    gix::objs::compute_hash(repo.object_hash(), gix::objs::Kind::Blob, &bytes).map(Some).map_err(read_failure)
}

/// 工作目錄的內容;symlink 是連結目標(與 git 存的 blob 相同)。
fn worktree_bytes(full: &Path, metadata: &std::fs::Metadata) -> std::io::Result<Vec<u8>> {
    if metadata.file_type().is_symlink() {
        return Ok(gix::path::os_string_into_bstring(std::fs::read_link(full)?.into_os_string()).map_err(std::io::Error::other)?.into());
    }
    std::fs::read(full)
}

/// 沒有加入 index、也沒被排除的檔案(依 .gitignore 等;被排除的資料夾整個略過，巢狀 repo 也略過)。
fn untracked(repository: &Repository, index: &gix::worktree::Index, tracked: &BTreeSet<&BString>) -> Result<Vec<String>, GitFailure> {
    let mut excludes = repository
        .repo
        .excludes(index, None, gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped)
        .map_err(read_failure)?;
    let mut found = Vec::new();
    let mut walked = 0;
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(repository.workdir.join(&relative)) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            walked += 1;
            if walked > MAXIMUM_WALKED_ENTRIES {
                return Ok(found);
            }
            let name = entry.file_name();
            if relative.as_os_str().is_empty() && name == ".git" {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = relative.join(&name);
            let mode = if file_type.is_dir() { gix::index::entry::Mode::DIR } else { gix::index::entry::Mode::FILE };
            if excludes.at_path(&path, Some(mode)).map_err(read_failure)?.is_excluded() {
                continue;
            }
            if file_type.is_dir() {
                if !entry.path().join(".git").exists() {
                    pending.push(path);
                }
                continue;
            }
            let Ok(relative_bytes) = gix::path::into_bstr(path.as_path()) else {
                continue;
            };
            let slashed = gix::path::to_unix_separators_on_windows(relative_bytes).into_owned();
            if !tracked.contains(&slashed) {
                found.push(slashed.to_string());
            }
        }
    }
    Ok(found)
}

/// 一個變更的 diff 文字，或不給 diff 的原因。
fn file_diff(repository: &Repository, change: &Change) -> Result<Result<String, GitDiffOmitted>, GitFailure> {
    let full = repository.workdir.join(&change.path);
    if host_files::is_excluded(&full) {
        return Ok(Err(GitDiffOmitted::Excluded));
    }
    let before = match change.head {
        Some(id) => {
            let object = repository.repo.find_object(id).map_err(read_failure)?;
            if object.data.len() as u64 > MAXIMUM_FILE_SIZE {
                return Ok(Err(GitDiffOmitted::TooLarge));
            }
            object.data.clone()
        }
        None => Vec::new(),
    };
    let after = match std::fs::symlink_metadata(&full) {
        Ok(metadata) if metadata.is_dir() => Vec::new(),
        Ok(metadata) if metadata.len() > MAXIMUM_FILE_SIZE => return Ok(Err(GitDiffOmitted::TooLarge)),
        Ok(metadata) => worktree_bytes(&full, &metadata).map_err(read_failure)?,
        Err(_) => Vec::new(),
    };
    let (Some(before), Some(after)) = (text(&before), text(&after)) else {
        return Ok(Err(GitDiffOmitted::Binary));
    };
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let printer = GitHunkPrinter(BasicLineDiffPrinter(&input.interner));
    Ok(Ok(diff.unified_diff(&printer, UnifiedDiffConfig::default(), &input).to_string()))
}

/// 可以當文字 diff 的內容：UTF-8 且沒有 NUL。
fn text(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok().filter(|text| !text.contains('\0'))
}

/// hunk 標頭照 git 的寫法：行數 0 時起點是前一行(`-0,0`),行數 1 時省略行數(`+1`)。內容行沿用 imara 的格式。
struct GitHunkPrinter<'a>(BasicLineDiffPrinter<'a, str>);

impl UnifiedDiffPrinter for GitHunkPrinter<'_> {
    fn display_header(&self, mut f: impl std::fmt::Write, start_before: u32, start_after: u32, len_before: u32, len_after: u32) -> std::fmt::Result {
        writeln!(f, "@@ -{} +{} @@", hunk_range(start_before, len_before), hunk_range(start_after, len_after))
    }

    fn display_context_token(&self, f: impl std::fmt::Write, token: Token) -> std::fmt::Result {
        self.0.display_context_token(f, token)
    }

    fn display_hunk(&self, f: impl std::fmt::Write, before: &[Token], after: &[Token]) -> std::fmt::Result {
        self.0.display_hunk(f, before, after)
    }
}

/// `start` 是 0 起算的行。
fn hunk_range(start: u32, len: u32) -> String {
    match len {
        0 => format!("{start},0"),
        1 => format!("{}", start + 1),
        _ => format!("{},{len}", start + 1),
    }
}
