//! 假的 Release 伺服器(本機 HTTP):`/releases/latest` 與 `/releases/download/host-service-v<版本>/<檔名>`,格式同 GitHub。

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use purrdr_host_service::install::InstallPlatform;
use sha2::{Digest, Sha256};

/// 這台(測試假裝的)主機該下載的壓縮檔名(獨立於實作的對照表)。
pub fn asset_name(platform: InstallPlatform) -> String {
    let architecture = std::env::consts::ARCH;
    match platform {
        InstallPlatform::MacOs => format!("purrdr-host-service-{architecture}-apple-darwin.tar.gz"),
        InstallPlatform::Linux => format!("purrdr-host-service-{architecture}-unknown-linux-musl.tar.gz"),
        InstallPlatform::Windows => "purrdr-host-service-x86_64-pc-windows-msvc.zip".to_owned(),
    }
}

/// 壓縮檔：Windows 是 zip,其他是 tar.gz;`files` 是檔名與內容。
pub fn archive(platform: InstallPlatform, files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    if platform == InstallPlatform::Windows {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, contents) in files {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap().into_inner()
    } else {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (name, contents) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, contents.as_slice()).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

type Files = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

/// 測試結束(drop)時停止的假 Release 伺服器。
pub struct FakeReleaseServer {
    address: String,
    files: Files,
    stopped: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl FakeReleaseServer {
    pub fn start() -> FakeReleaseServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let files: Files = Arc::default();
        let stopped = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (thread_files, thread_stopped, thread_requests) = (files.clone(), stopped.clone(), requests.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stopped.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = stream {
                    respond(stream, &thread_files, &thread_requests);
                }
            }
        });
        FakeReleaseServer {
            address,
            files,
            stopped,
            requests,
        }
    }

    /// 傳給 `HttpReleaseSource::at` 的位址。
    pub fn url(&self) -> String {
        self.address.clone()
    }

    /// 發佈 `version`:最新版、壓縮檔(內含 `executables`,內容是各自的檔名對應的 `contents`)與 `SHA256SUMS`。
    pub fn publish(&self, platform: InstallPlatform, version: &str, executables: &[(&str, String)]) {
        let files: Vec<(&str, Vec<u8>)> = executables.iter().map(|(name, contents)| (*name, contents.clone().into_bytes())).collect();
        let archive = archive(platform, &files);
        let checksums = format!(
            "{}  purrdr-host-service-other-target.tar.gz\n{}  {}\n",
            "0".repeat(64),
            sha256_hex(&archive),
            asset_name(platform)
        );
        self.put("releases/latest", format!(r#"{{"tag_name":"host-service-v{version}"}}"#).into_bytes());
        self.put(&format!("releases/download/host-service-v{version}/{}", asset_name(platform)), archive);
        self.put(&format!("releases/download/host-service-v{version}/SHA256SUMS"), checksums.into_bytes());
    }

    /// 改寫某個檔案(例如讓 `SHA256SUMS` 不符)。
    pub fn put(&self, path: &str, contents: Vec<u8>) {
        self.files.lock().unwrap().insert(format!("/{path}"), contents);
    }

    /// 收到的請求路徑。
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeReleaseServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address.trim_start_matches("http://"));
    }
}

fn respond(mut stream: TcpStream, files: &Files, requests: &Mutex<Vec<String>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
            break;
        }
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_owned();
    requests.lock().unwrap().push(path.clone());
    let response = files.lock().unwrap().get(&path).cloned();
    let (status, body) = match response {
        Some(body) => ("200 OK", body),
        None => ("404 Not Found", b"Not Found".to_vec()),
    };
    let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
}
