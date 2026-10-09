//! easyS3 Tauri 胶水层：命令注册、插件、状态初始化。
//! 业务逻辑全部在 easys3-core（可独立测试），本层只做 IPC 编排。

mod commands;
mod secrets;
mod state;
mod tasks;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tauri::Manager;
            let app_state = state::App::init(app.handle());
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::save_connection,
            commands::delete_connection,
            commands::set_current_connection,
            commands::test_connection,
            commands::list_buckets,
            commands::list_objects,
            commands::count_objects,
            commands::preview_object,
            commands::is_download_directory,
            commands::object_detail,
            commands::create_folder,
            commands::start_copy,
            commands::list_multipart_uploads,
            commands::abort_multipart_uploads,
            commands::list_multipart_parts,
            commands::plan_upload,
            commands::start_upload,
            commands::start_download,
            commands::start_delete,
            commands::cancel_task,
            commands::retry_task,
            commands::get_tasks,
            commands::clear_finished_tasks,
            commands::get_settings,
            commands::save_transfer_settings,
            commands::open_object
        ])
        .build(tauri::generate_context!())
        .expect("easyS3 运行失败")
        .run(|_app, event| {
            // 退出时清理"打开"临时文件（P0-10）
            if let tauri::RunEvent::Exit = event {
                let _ = std::fs::remove_dir_all(state::open_temp_dir());
            }
        });
}
