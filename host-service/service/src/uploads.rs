//! 上傳圖片與檔案(raychiutw/purrdr.host#16):分塊寫到家目錄的 `.purrdr/uploads/`(與 app 的 SSH 上傳同一個資料夾),
//! 檔名由主機服務產生。

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::task::JoinHandle;

use crate::protocol::{MAXIMUM_FILE_CHUNK, MAXIMUM_FILE_SIZE};

/// 家目錄底下的上傳資料夾。
pub const UPLOAD_FOLDER: [&str; 2] = [".purrdr", "uploads"];

/// 上傳檔保留多久(與 SSH 上傳的 `find -mmin +10080` 相同)。
pub const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// 定期清理的間隔。
pub const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// 啟動時與之後每 `CLEANUP_INTERVAL` 清掉超過 7 天的上傳檔;`clock` 提供現在時間(測試注入)。
pub fn spawn_cleanup(directory: UploadDirectory, clock: impl Fn() -> SystemTime + Send + 'static) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            // 資料夾只有少量檔案，直接在這裡做。
            if let Err(error) = directory.remove_expired(clock()) {
                eprintln!("cannot clean uploads {}: {error}", directory.path.display());
            }
            tokio::time::sleep(CLEANUP_INTERVAL).await;
        }
    })
}

/// 家目錄的 `.purrdr/uploads/`。
#[derive(Debug, Clone)]
pub struct UploadDirectory {
    path: PathBuf,
}

impl UploadDirectory {
    pub fn under_home(home: &Path) -> Self {
        UploadDirectory {
            path: UPLOAD_FOLDER.iter().fold(home.to_owned(), |path, segment| path.join(segment)),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 刪掉修改時間比 `now` 早超過 7 天的檔案。
    pub fn remove_expired(&self, now: SystemTime) -> io::Result<()> {
        let entries = match std::fs::read_dir(&self.path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let metadata = entry.metadata()?;
            let expired = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > RETENTION);
            if metadata.is_file() && expired {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        Ok(())
    }

    /// 開始一次上傳：在上傳資料夾建隱藏的暫存檔。
    pub(crate) fn begin(&self, size: u64, extension: Option<&str>) -> Result<PendingUpload, UploadFailure> {
        if size > MAXIMUM_FILE_SIZE {
            return Err(UploadFailure::TooLarge(format!("upload is larger than {MAXIMUM_FILE_SIZE} bytes")));
        }
        let extension = match extension {
            Some(extension) if is_valid_extension(extension) => Some(extension.to_ascii_lowercase()),
            Some(_) => return Err(UploadFailure::Invalid("extension must be 1-16 ASCII letters or digits")),
            None => None,
        };
        create_private_directory(&self.path).map_err(UploadFailure::Storage)?;
        let temporary = self.path.join(format!(".upload.{}", random_base36(16).map_err(UploadFailure::Storage)?));
        let file = create_private_file(&temporary).map_err(UploadFailure::Storage)?;
        Ok(PendingUpload {
            directory: self.path.clone(),
            temporary,
            file: Some(file),
            extension,
            size,
            received: 0,
        })
    }
}

/// 上傳失敗的原因。
#[derive(Debug)]
pub(crate) enum UploadFailure {
    /// 超過 `x-maximum-file-size` 或 `x-maximum-file-chunk`。
    TooLarge(String),
    /// offset 不連續或超過宣告的大小。
    Invalid(&'static str),
    Storage(io::Error),
}

impl UploadFailure {
    /// 稽核紀錄的 result。
    pub fn audit_result(&self) -> &'static str {
        match self {
            UploadFailure::TooLarge(_) => "too_large",
            UploadFailure::Invalid(_) => "invalid",
            UploadFailure::Storage(_) => "failed",
        }
    }
}

/// 一個分塊寫入後的狀態。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ChunkOutcome {
    Partial { received: u64 },
    Stored { received: u64, path: PathBuf },
}

/// 進行中的上傳;drop 時還沒存成正式檔就刪掉暫存檔。
#[derive(Debug)]
pub(crate) struct PendingUpload {
    directory: PathBuf,
    temporary: PathBuf,
    file: Option<File>,
    extension: Option<String>,
    size: u64,
    received: u64,
}

impl PendingUpload {
    /// 宣告的總大小。
    pub fn size(&self) -> u64 {
        self.size
    }

    /// 依序寫入一塊;收滿時換成主機產生的正式檔名。
    pub fn append(&mut self, offset: u64, bytes: &[u8]) -> Result<ChunkOutcome, UploadFailure> {
        if bytes.len() as u64 > MAXIMUM_FILE_CHUNK {
            return Err(UploadFailure::TooLarge(format!("chunk is larger than {MAXIMUM_FILE_CHUNK} bytes")));
        }
        if offset != self.received {
            return Err(UploadFailure::Invalid("offset does not match the bytes received so far"));
        }
        if self.received + bytes.len() as u64 > self.size {
            return Err(UploadFailure::Invalid("chunk goes past the declared size"));
        }
        let file = self.file.as_mut().expect("上傳完成後不再寫入");
        file.write_all(bytes).map_err(UploadFailure::Storage)?;
        self.received += bytes.len() as u64;
        if self.received < self.size {
            return Ok(ChunkOutcome::Partial { received: self.received });
        }
        let file = self.file.take().expect("剛寫入的暫存檔");
        file.sync_all().map_err(UploadFailure::Storage)?;
        drop(file);
        let mut name = stored_name(SystemTime::now()).map_err(UploadFailure::Storage)?;
        if let Some(extension) = &self.extension {
            name = format!("{name}.{extension}");
        }
        let path = self.directory.join(name);
        std::fs::rename(&self.temporary, &path).map_err(UploadFailure::Storage)?;
        Ok(ChunkOutcome::Stored { received: self.received, path })
    }
}

impl Drop for PendingUpload {
    fn drop(&mut self) {
        // 先關檔:Windows 不能刪開著的檔案。
        if let Some(file) = self.file.take() {
            drop(file);
            let _ = std::fs::remove_file(&self.temporary);
        }
    }
}

/// `<時間(毫秒)base36>-<8 個 base36 亂數>`,與 app 的 SSH 上傳檔名同格式。
fn stored_name(now: SystemTime) -> io::Result<String> {
    let millis = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    Ok(format!("{}-{}", to_base36(millis), random_base36(8)?))
}

fn is_valid_extension(extension: &str) -> bool {
    (1..=16).contains(&extension.len()) && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

const BASE36_DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn to_base36(mut value: u128) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(BASE36_DIGITS[(value % 36) as usize]);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    digits.reverse();
    String::from_utf8(digits).expect("base36 是 ASCII")
}

fn random_base36(length: usize) -> io::Result<String> {
    let mut bytes = vec![0u8; length];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(bytes.iter().map(|byte| BASE36_DIGITS[usize::from(*byte) % 36] as char).collect())
}

/// 只有本人可讀(Unix 700,與 SSH 上傳相同)。
fn create_private_directory(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
