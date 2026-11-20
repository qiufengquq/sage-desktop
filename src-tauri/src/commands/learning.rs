use serde_json::Value;
use tauri::AppHandle;

use crate::commands::{blocking, db};
use crate::error::{AppError, AppResult};
use crate::models::assessment::{AssessmentModule, AssessmentOutcome};
use crate::models::exercise::{
    AnswerCheck, ExerciseAnswer, ExerciseResult, TrainingOption, TrainingSession,
};
use crate::models::review::{ReviewQueuePayload, ReviewSubmitResult};
use crate::services::{answer_service, assessment_service, review_service, training_service};

/// 学习总览。第二阶段的实现会在这里聚合 SQLite 中的每日记录。
#[tauri::command]
pub fn dashboard_snapshot() -> AppResult<Value> {
    Err(AppError::unavailable("学习总览"))
}

/* --------------------------------- 数据分析 -------------------------------- */

#[tauri::command]
pub fn analytics_query(range: Option<u16>) -> AppResult<Value> {
    let _ = range;
    Err(AppError::unavailable("学习数据聚合"))
}

/* --------------------------------- AI 规划 -------------------------------- */

#[tauri::command]
pub fn plan_get() -> AppResult<Value> {
    Err(AppError::unavailable("AI 学习计划"))
}

#[tauri::command]
pub fn plan_regenerate() -> AppResult<Value> {
    Err(AppError::unavailable("AI 计划生成"))
}

#[tauri::command]
pub fn plan_apply_difficulty(level: f32) -> AppResult<Value> {
    let _ = level;
    Err(AppError::unavailable("难度调整"))
}

#[tauri::command]
pub fn plan_toggle_item(item_id: String) -> AppResult<Value> {
    let _ = item_id;
    Err(AppError::unavailable("计划项状态"))
}

/* ---------------------------------- 测评 ---------------------------------- */

#[tauri::command]
pub async fn assessment_modules(app: AppHandle) -> AppResult<Vec<AssessmentModule>> {
    let db = db(&app);
    blocking(move || db.with_conn(assessment_service::modules)).await
}

/// 提交测评：判分以库内题目为准，`submissionId` **必填**（幂等键）
#[tauri::command]
pub async fn assessment_submit(
    app: AppHandle,
    module_id: String,
    answers: Vec<ExerciseAnswer>,
    submission_id: String,
) -> AppResult<AssessmentOutcome> {
    let db = db(&app);
    blocking(move || {
        db.with_tx(|tx| assessment_service::submit(tx, &module_id, &answers, &submission_id))
    })
    .await
}

/// 测评中的逐题即时反馈：只判定该模块内的题目，答案由服务端回传
#[tauri::command]
pub async fn assessment_check(
    app: AppHandle,
    module_id: String,
    exercise_id: String,
    value: Value,
) -> AppResult<AnswerCheck> {
    let db = db(&app);
    blocking(move || {
        db.with_conn(|conn| answer_service::check_assessment(conn, &module_id, &exercise_id, &value))
    })
    .await
}

/* ---------------------------------- 训练 ---------------------------------- */

#[tauri::command]
pub async fn training_options(
    app: AppHandle,
    track: Option<String>,
) -> AppResult<Vec<TrainingOption>> {
    let db = db(&app);
    blocking(move || db.with_conn(|conn| training_service::options(conn, track.as_deref()))).await
}

/// 开始训练：按题型取题并把题目快照落库，提交时据此判分
#[tauri::command]
pub async fn training_start(
    app: AppHandle,
    kind: String,
    track: String,
    count: Option<i64>,
    title: Option<String>,
    difficulty: Option<f64>,
) -> AppResult<TrainingSession> {
    let db = db(&app);
    blocking(move || {
        db.with_tx(|tx| {
            training_service::start(
                tx,
                training_service::StartInput {
                    kind,
                    track,
                    count,
                    title,
                    difficulty,
                },
            )
        })
    })
    .await
}

/// 提交训练：只接受 `sessionId + answers`，题目与答案以服务端快照为准
#[tauri::command]
pub async fn training_submit(
    app: AppHandle,
    session_id: String,
    answers: Vec<ExerciseAnswer>,
) -> AppResult<ExerciseResult> {
    let db = db(&app);
    blocking(move || db.with_tx(|tx| training_service::submit(tx, &session_id, &answers))).await
}

/// 训练中的逐题即时反馈：题目必须来自该会话快照
#[tauri::command]
pub async fn training_check(
    app: AppHandle,
    session_id: String,
    exercise_id: String,
    value: Value,
) -> AppResult<AnswerCheck> {
    let db = db(&app);
    blocking(move || {
        db.with_conn(|conn| answer_service::check_training(conn, &session_id, &exercise_id, &value))
    })
    .await
}

/* ---------------------------------- 复习 ---------------------------------- */

/// 领取到期复习队列：内容 + 调度预览都按当前状态现算
#[tauri::command]
pub async fn review_queue(app: AppHandle, limit: Option<i64>) -> AppResult<ReviewQueuePayload> {
    let db = db(&app);
    blocking(move || db.with_conn(|conn| review_service::queue(conn, limit.unwrap_or(20)))).await
}

/// 提交一次自评：排期、复习日志、掌握度账本与 XP 在同一个事务里完成
#[tauri::command]
pub async fn review_submit(
    app: AppHandle,
    entity_id: String,
    rating: String,
    submission_id: Option<String>,
) -> AppResult<ReviewSubmitResult> {
    let db = db(&app);
    blocking(move || {
        db.with_tx(|tx| {
            review_service::submit(tx, &entity_id, &rating, submission_id.as_deref())
        })
    })
    .await
}
