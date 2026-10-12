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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            use tauri::{
                menu::{Menu, MenuItem},
                tray::TrayIconBuilder,
                Manager,
            };
            let app_state = state::App::init(app.handle());
            app.manage(app_state);
            let show = MenuItem::with_id(app, "tray-show", "显示窗口", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "tray-quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("easyS3")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "tray-show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "tray-quit" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.close();
                        }
                    }
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::test_assume_role,
            commands::start_sso_device_authorization,
            commands::complete_sso_device_authorization,
            commands::activate_assume_role,
            commands::assume_role_expiry,
            commands::get_audit,
            commands::export_audit,
            commands::record_audit,
            commands::cost_overview,
            commands::cost_by_service,
            commands::cost_by_usage_type,
            commands::generate_iac,
            commands::import_inventory,
            commands::search_inventory,
            commands::generate_sdk_snippet,
            commands::open_browser_window,
            commands::export_connections,
            commands::import_connections,
            commands::import_aws_profiles,
            commands::get_resumable_tasks,
            commands::discard_resumable_task,
            commands::resume_task,
            commands::save_connection,
            commands::delete_connection,
            commands::set_current_connection,
            commands::test_connection,
            commands::list_buckets,
            commands::create_bucket,
            commands::delete_bucket,
            commands::bucket_versioning,
            commands::set_bucket_versioning,
            commands::bucket_policy,
            commands::save_bucket_policy,
            commands::bucket_cors,
            commands::save_bucket_cors,
            commands::bucket_lifecycle,
            commands::save_bucket_lifecycle,
            commands::bucket_website,
            commands::save_bucket_website,
            commands::object_lock,
            commands::select_csv_object,
            commands::start_select_csv_object,
            commands::select_json_object,
            commands::start_select_json_object,
            commands::list_objects,
            commands::count_objects,
            commands::prefix_usage,
            commands::start_prefix_usage,
            commands::plan_sync_upload,
            commands::plan_sync_download,
            commands::start_search,
            commands::start_sync_upload,
            commands::start_sync_download,
            commands::start_zip_download,
            commands::preview_object,
            commands::thumbnail_object,
            commands::is_download_directory,
            commands::presign_get,
            commands::presign_put,
            commands::export_presign_list,
            commands::object_detail,
            commands::restore_archived_object,
            commands::object_versions,
            commands::object_tags,
            commands::save_object_tags,
            commands::object_acl,
            commands::restore_object_version,
            commands::replace_object_metadata,
            commands::save_text_object,
            commands::create_folder,
            commands::start_copy,
            commands::list_multipart_uploads,
            commands::abort_multipart_uploads,
            commands::list_multipart_parts,
            commands::plan_upload,
            commands::paste_clipboard_image,
            commands::plan_upload_changes,
            commands::start_upload,
            commands::start_upload_plan,
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
