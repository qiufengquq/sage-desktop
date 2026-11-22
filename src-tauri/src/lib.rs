pub mod ai;
pub mod algorithms;
pub mod commands;
pub mod database;
pub mod error;
pub mod models;
pub mod repositories;
pub mod services;
pub mod state;

use ai::commands as ai_commands;
use commands::{app, knowledge, learning, user};
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // 数据库初始化放在 setup 里：失败就直接退出，避免半可用状态
            let state = AppState::init(app.handle())
                .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 应用
            app::app_health,
            app::app_info,
            // 用户与设置（已接入 SQLite）
            user::settings_get,
            user::settings_update,
            user::user_profile_get,
            user::user_profile_update,
            user::sync_status_get,
            user::sync_trigger,
            user::local_data_summary,
            user::local_data_clear,
            // 学习总览与数据
            learning::dashboard_snapshot,
            learning::analytics_query,
            // 学习计划
            learning::plan_get,
            learning::plan_regenerate,
            learning::plan_apply_difficulty,
            learning::plan_toggle_item,
            // 测评
            learning::assessment_modules,
            learning::assessment_submit,
            learning::assessment_check,
            // 训练
            learning::training_options,
            learning::training_start,
            learning::training_submit,
            learning::training_check,
            // 复习
            learning::review_queue,
            learning::review_submit,
            // 知识库
            knowledge::knowledge_graph,
            knowledge::knowledge_list,
            knowledge::knowledge_detail,
            knowledge::knowledge_neighbors,
            // SAGE Intelligence（Library 集成）
            ai_commands::ai_status,
            ai_commands::ai_version,
            ai_commands::ai_provider_upsert,
            ai_commands::ai_provider_delete,
            ai_commands::ai_provider_test,
            ai_commands::ai_complete,
            ai_commands::ai_stream_start,
            ai_commands::ai_stream_cancel,
        ])
        .build(tauri::generate_context!())
        .expect("Sage 启动失败")
        .run(|app, event| {
            // 退出时只标记状态，不强制中断在途请求，避免拖慢关闭流程
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().ai.shutdown();
            }
        });
}
