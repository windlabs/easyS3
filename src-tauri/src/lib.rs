//! easyS3 Tauri 胶水层：命令注册、插件、状态初始化。
//! 业务逻辑全部在 easys3-core（可独立测试），本层只做 IPC 编排。

mod commands;
mod state;
mod tasks;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            use tauri::Manager;
            let app_state = state::App::init(app.handle());
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::save_project,
            commands::delete_project,
            commands::set_current_project,
            commands::test_project_connection,
            commands::list_buckets,
            commands::list_objects,
            commands::count_objects,
            commands::preview_object,
            commands::plan_upload,
            commands::start_upload,
            commands::start_download,
            commands::start_delete,
            commands::cancel_task,
            commands::retry_task,
            commands::get_tasks,
            commands::clear_finished_tasks
        ])
        .run(tauri::generate_context!())
        .expect("easyS3 运行失败");
}
