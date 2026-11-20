use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub stage: &'static str,
    pub data_source: &'static str,
}

/// 健康检查：前端在启动时可以调用它确认后端可用。
#[tauri::command]
pub fn app_health() -> bool {
    true
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "Sage",
        version: "0.1.0",
        stage: "phase-2-sqlite",
        data_source: "backend",
    }
}
