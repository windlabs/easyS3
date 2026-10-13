// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::panic::{catch_unwind, AssertUnwindSafe};

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "未知启动异常".to_string()
    }
}

fn report_startup_failure(message: &str) {
    let detail = format!(
        "easyS3 启动失败。\n\n错误：{message}\n\n请确认已安装 Microsoft Edge WebView2 Runtime（Evergreen x64），然后重试。\n诊断日志：{}",
        std::env::temp_dir().join("easyS3-startup.log").display()
    );
    let log = format!("easyS3 startup failure\n{detail}\n");
    let path = std::env::temp_dir().join("easyS3-startup.log");
    let _ = std::fs::write(path, log);

    #[cfg(target_os = "windows")]
    show_windows_error(&detail);
    #[cfg(not(target_os = "windows"))]
    eprintln!("{detail}");
}

#[cfg(target_os = "windows")]
fn show_windows_error(message: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            hwnd: *mut std::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let caption: Vec<u16> = "easyS3 启动失败"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both UTF-16 buffers are NUL-terminated and valid for this call.
    unsafe {
        let _ = MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x10);
    }
}

fn main() {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(easys3_lib::run)) {
        report_startup_failure(&panic_text(payload));
    }
}
