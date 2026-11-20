//! 用户资料与设置。对应 `src/types/user.ts`。

use serde::{Deserialize, Serialize};

/// 本地默认用户（单机版只有一行）
pub const LOCAL_USER_ID: &str = "u_local_01";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: String,
    pub display_name: String,
    pub handle: String,
    pub joined_at: String,
    pub level: String,
    pub target_level: String,
    pub goal: String,
    pub daily_minutes: i64,
    pub weekly_days: i64,
    pub timezone: String,
}

impl UserProfile {
    /// 首次启动的默认资料，与第一阶段 mock 保持一致（此处是唯一来源）
    pub fn default_for(id: &str, now: &str) -> Self {
        Self {
            id: id.to_string(),
            display_name: "学习者".to_string(),
            handle: "@local".to_string(),
            joined_at: now.to_string(),
            level: "B1".to_string(),
            target_level: "B2".to_string(),
            goal: "exam".to_string(),
            daily_minutes: 40,
            weekly_days: 6,
            timezone: "Asia/Shanghai".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    pub theme: String,
    pub accent: String,
    pub density: String,
    pub reduce_motion: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            accent: "brand".to_string(),
            density: "comfortable".to_string(),
            reduce_motion: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderSettings {
    pub enabled: bool,
    /// HH:mm
    pub time: String,
    pub weekdays_only: bool,
}

impl Default for ReminderSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            time: "20:30".to_string(),
            weekdays_only: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub appearance: AppearanceSettings,
    pub reminder: ReminderSettings,
    pub sound_effects: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            appearance: AppearanceSettings::default(),
            reminder: ReminderSettings::default(),
            sound_effects: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub state: String,
    pub last_synced_at: Option<String>,
    pub pending_changes: i64,
    pub device_name: String,
    pub storage_used_kb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDataSummary {
    pub words: i64,
    pub grammar_points: i64,
    pub sentences: i64,
    pub records: i64,
    pub size_kb: u64,
}
