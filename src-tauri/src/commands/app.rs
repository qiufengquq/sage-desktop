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

/// 版本号只有一个来源：`Cargo.toml` 的 `package.version`（与 `tauri.conf.json`、
/// `package.json` 必须一致）。禁止在代码里写死版本字符串。
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "Sage",
        version: env!("CARGO_PKG_VERSION"),
        stage: "release-1.0.1",
        data_source: "backend",
    }
}
