//! 背景啟動的程式不持有呼叫端的 stdio(raychiutw/purrdr.host#35):`update --yes 2>&1 | tail` 在 Windows 上因為
//! 重啟的背景服務繼承了管線的 handle,update 結束後讀端一直等不到 EOF。
//!
//! 這支測試執行檔本身帶 `PURRDR_TEST_SPAWN_DETACHED` 時是「呼叫端」:以 `spawn_detached` 啟動一支會跑一陣子的程式後
//! 立刻結束。測試把呼叫端的 stdout 接成管線，呼叫端結束後應該很快讀到 EOF。

use std::io::Read;
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use purrdr_host_service::install::system::{HostSystem, ProcessHostSystem};

const CALLER: &str = "PURRDR_TEST_SPAWN_DETACHED";

fn main() -> ExitCode {
    if std::env::var_os(CALLER).is_some() {
        return spawn_long_running_program();
    }
    let started = std::time::Instant::now();
    the_caller_pipe_closes_when_the_caller_exits();
    println!("test the_caller_pipe_closes_when_the_caller_exits ... ok ({:?})", started.elapsed());
    ExitCode::SUCCESS
}

/// 會跑 20 秒的程式(系統內建，不讀 stdin)。
fn spawn_long_running_program() -> ExitCode {
    let (program, arguments): (&str, Vec<String>) = if cfg!(windows) {
        ("ping.exe", vec!["-n".into(), "20".into(), "127.0.0.1".into()])
    } else {
        ("sleep", vec!["20".into()])
    };
    ProcessHostSystem.spawn_detached(program, &arguments).expect("啟動背景程式");
    println!("spawned");
    ExitCode::SUCCESS
}

fn the_caller_pipe_closes_when_the_caller_exits() {
    let mut caller = Command::new(std::env::current_exe().unwrap())
        .env(CALLER, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = caller.stdout.take().unwrap();
    let mut stderr = caller.stderr.take().unwrap();
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = String::new();
        let _ = stdout.read_to_string(&mut output);
        let mut errors = String::new();
        let _ = stderr.read_to_string(&mut errors);
        let _ = done.send((output, errors));
    });
    let status = caller.wait().unwrap();
    assert!(status.success(), "呼叫端失敗:{status}");

    let (output, errors) = finished
        .recv_timeout(Duration::from_secs(10))
        .expect("呼叫端結束 10 秒後管線仍沒有 EOF:背景程式繼承了呼叫端的 stdout／stderr");
    assert_eq!(output.trim(), "spawned", "stderr:{errors}");
}
