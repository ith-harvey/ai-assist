//! REST API routes for households, members, and tasks.
//!
//! Endpoints:
//! - `POST   /api/households`                              — create a household
//! - `GET    /api/households`                              — list households for user
//! - `GET    /api/households/:id`                          — get a household
//! - `PUT    /api/households/:id`                          — update a household
//! - `DELETE /api/households/:id`                          — delete a household
//! - `POST   /api/households/:id/members`                  — add a member
//! - `GET    /api/households/:id/members`                  — list members
//! - `DELETE /api/households/:id/members/:user_id`         — remove a member
//! - `POST   /api/households/:id/tasks`                    — create a task
//! - `GET    /api/households/:id/tasks`                    — list tasks
//! - `GET    /api/households/:id/tasks/:task_id`           — get a task
//! - `PUT    /api/households/:id/tasks/:task_id`           — update a task
//! - `DELETE /api/households/:id/tasks/:task_id`           — delete a task
//! - `GET    /api/households/tasks/mine`                   — list tasks assigned to me
//! - `GET    /ws/households/:id/tasks`                     — WebSocket for real-time task updates

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use serde::Deserialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::model::*;
use crate::context::AppContext;

/// Query params for listing tasks.
#[derive(Debug, Deserialize)]
pub struct ListTasksParams {
    pub status: Option<String>,
}

/// Request body for creating a household.
#[derive(Debug, Deserialize)]
pub struct CreateHouseholdRequest {
    pub name: String,
}

/// Request body for updating a household.
#[derive(Debug, Deserialize)]
pub struct UpdateHouseholdRequest {
    pub name: Option<String>,
}

/// Request body for adding a member.
#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    pub user_id: String,
    pub display_name: String,
    #[serde(default)]
    pub role: Option<HouseholdRole>,
}

/// Request body for creating a task.
#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: Option<HouseholdTaskPriority>,
    #[serde(default)]
    pub assigned_to: Option<String>,
    #[serde(default)]
    pub due_date: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub recurrence: Option<RecurrenceRule>,
}

/// Request body for updating a task.
#[derive(Debug, Deserialize)]
pub struct UpdateTaskRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<HouseholdTaskStatus>,
    #[serde(default)]
    pub priority: Option<HouseholdTaskPriority>,
    #[serde(default)]
    pub assigned_to: Option<String>,
    #[serde(default)]
    pub due_date: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub recurrence: Option<RecurrenceRule>,
}

/// Build the Axum router for `/api/households`.
pub fn household_routes(ctx: Arc<AppContext>) -> Router {
    Router::new()
        // Household CRUD
        .route("/api/households", post(create_household).get(list_households))
        .route(
            "/api/households/{id}",
            get(get_household).put(update_household).delete(delete_household),
        )
        // Members
        .route(
            "/api/households/{id}/members",
            post(add_member).get(list_members),
        )
        .route(
            "/api/households/{id}/members/{user_id}",
            delete(remove_member),
        )
        // Tasks
        .route("/api/households/tasks/mine", get(list_my_tasks))
        .route(
            "/api/households/{id}/tasks",
            post(create_task).get(list_tasks),
        )
        .route(
            "/api/households/{id}/tasks/{task_id}",
            get(get_task).put(update_task).delete(delete_task),
        )
        // WebSocket
        .route("/ws/households/{id}/tasks", get(ws_handler))
        .with_state(ctx)
}

// ── Household handlers ─────────────────────────────────────────────

async fn create_household(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<CreateHouseholdRequest>,
) -> impl IntoResponse {
    let user_id = "default".to_string();
    let household = Household::new(req.name, &user_id);
    let household_id = household.id;

    if let Err(e) = ctx.db.create_household(&household).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response();
    }

    // Auto-add creator as owner
    let member = HouseholdMember::new(household_id, &user_id, "Owner", HouseholdRole::Owner);
    if let Err(e) = ctx.db.add_household_member(&member).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response();
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({"id": household_id.to_string(), "household": household})),
    )
        .into_response()
}

async fn list_households(State(ctx): State<Arc<AppContext>>) -> impl IntoResponse {
    let user_id = "default";
    match ctx.db.list_households_for_user(user_id).await {
        Ok(households) => Json(serde_json::json!({"households": households})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_household(
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let household_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid household ID"})),
            )
                .into_response()
        }
    };

    match ctx.db.get_household(household_id).await {
        Ok(Some(h)) => Json(h).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Household not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn update_household(
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateHouseholdRequest>,
) -> impl IntoResponse {
    let household_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid household ID"})),
            )
                .into_response()
        }
    };

    let existing = match ctx.db.get_household(household_id).await {
        Ok(Some(h)) => h,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Household not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let updated = Household {
        name: req.name.unwrap_or(existing.name),
        updated_at: chrono::Utc::now(),
        ..existing
    };

    match ctx.db.update_household(&updated).await {
        Ok(()) => Json(serde_json::json!({"household": updated})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_household(
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let household_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid household ID"})),
            )
                .into_response()
        }
    };

    match ctx.db.delete_household(household_id).await {
        Ok(true) => Json(serde_json::json!({"deleted": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Household not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── Member handlers ────────────────────────────────────────────────

async fn add_member(
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
    Json(req): Json<AddMemberRequest>,
) -> impl IntoResponse {
    let household_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid household ID"})),
            )
                .into_response()
        }
    };

    let role = req.role.unwrap_or(HouseholdRole::Member);
    let member = HouseholdMember::new(household_id, req.user_id, req.display_name, role);
    let member_id = member.id;

    match ctx.db.add_household_member(&member).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id": member_id.to_string(), "member": member})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn list_members(
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let household_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid household ID"})),
            )
                .into_response()
        }
    };

    match ctx.db.list_household_members(household_id).await {
        Ok(members) => Json(serde_json::json!({"members": members})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn remove_member(
    State(ctx): State<Arc<AppContext>>,
    Path((id, user_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let household_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid household ID"})),
            )
                .into_response()
        }
    };

    match ctx.db.remove_household_member(household_id, &user_id).await {
        Ok(true) => Json(serde_json::json!({"removed": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Member not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── Task handlers ──────────────────────────────────────────────────

async fn create_task(
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
    Json(req): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let household_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid household ID"})),
            )
                .into_response()
        }
    };

    let user_id = "default";
    let mut task = HouseholdTask::new(household_id, req.title, user_id);
    if let Some(desc) = req.description {
        task = task.with_description(desc);
    }
    if let Some(priority) = req.priority {
        task = task.with_priority(priority);
    }
    if let Some(assigned) = req.assigned_to {
        task = task.with_assigned_to(assigned);
    }
    if let Some(due) = req.due_date {
        task = task.with_due_date(due);
    }
    if let Some(recurrence) = req.recurrence {
        task = task.with_recurrence(recurrence);
    }

    let task_id = task.id;
    match ctx.db.create_household_task(&task).await {
        Ok(()) => {
            let _ = ctx.household_tx.send(HouseholdWsMessage::TaskCreated {
                task: task.clone(),
            });
            (
                StatusCode::CREATED,
                Json(serde_json::json!({"id": task_id.to_string(), "task": task})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn list_tasks(
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
    Query(params): Query<ListTasksParams>,
) -> impl IntoResponse {
    let household_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid household ID"})),
            )
                .into_response()
        }
    };

    let status: Option<HouseholdTaskStatus> = params
        .status
        .and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok());

    match ctx
        .db
        .list_household_tasks(household_id, status.as_ref())
        .await
    {
        Ok(tasks) => Json(serde_json::json!({"tasks": tasks})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_task(
    State(ctx): State<Arc<AppContext>>,
    Path((_id, task_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let task_uuid = match Uuid::parse_str(&task_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid task ID"})),
            )
                .into_response()
        }
    };

    match ctx.db.get_household_task(task_uuid).await {
        Ok(Some(task)) => Json(task).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Task not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn update_task(
    State(ctx): State<Arc<AppContext>>,
    Path((_id, task_id)): Path<(String, String)>,
    Json(req): Json<UpdateTaskRequest>,
) -> impl IntoResponse {
    let task_uuid = match Uuid::parse_str(&task_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid task ID"})),
            )
                .into_response()
        }
    };

    let existing = match ctx.db.get_household_task(task_uuid).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Task not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let now = chrono::Utc::now();
    let new_status = req.status.unwrap_or(existing.status.clone());
    let completed_at = if new_status == HouseholdTaskStatus::Completed && existing.completed_at.is_none() {
        Some(now)
    } else if new_status != HouseholdTaskStatus::Completed {
        None
    } else {
        existing.completed_at
    };

    let updated = HouseholdTask {
        title: req.title.unwrap_or(existing.title),
        description: req.description.or(existing.description),
        status: new_status,
        priority: req.priority.unwrap_or(existing.priority),
        assigned_to: req.assigned_to.or(existing.assigned_to),
        due_date: req.due_date.or(existing.due_date),
        recurrence: req.recurrence.or(existing.recurrence),
        updated_at: now,
        completed_at,
        ..existing
    };

    match ctx.db.update_household_task(&updated).await {
        Ok(()) => {
            let _ = ctx.household_tx.send(HouseholdWsMessage::TaskUpdated {
                task: updated.clone(),
            });
            Json(serde_json::json!({"task": updated})).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_task(
    State(ctx): State<Arc<AppContext>>,
    Path((_id, task_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let task_uuid = match Uuid::parse_str(&task_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid task ID"})),
            )
                .into_response()
        }
    };

    match ctx.db.delete_household_task(task_uuid).await {
        Ok(true) => {
            let hid = Uuid::parse_str(&_id).unwrap_or_default();
            let _ = ctx
                .household_tx
                .send(HouseholdWsMessage::TaskDeleted { id: task_uuid, household_id: hid });
            Json(serde_json::json!({"deleted": true})).into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Task not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn list_my_tasks(State(ctx): State<Arc<AppContext>>) -> impl IntoResponse {
    let user_id = "default";
    match ctx.db.list_tasks_for_user(user_id).await {
        Ok(tasks) => Json(serde_json::json!({"tasks": tasks})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── WebSocket handler ──────────────────────────────────────────────

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(ctx): State<Arc<AppContext>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let household_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid household ID"})),
            )
                .into_response()
        }
    };

    ws.on_upgrade(move |socket| handle_socket(socket, ctx, household_id))
        .into_response()
}

async fn handle_socket(mut socket: WebSocket, ctx: Arc<AppContext>, household_id: Uuid) {
    // Send initial sync
    if let Ok(tasks) = ctx.db.list_household_tasks(household_id, None).await {
        let msg = HouseholdWsMessage::TasksSync {
            household_id,
            tasks,
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            if socket.send(Message::Text(json.into())).await.is_err() {
                return;
            }
        }
    }

    // Subscribe to broadcast
    let mut rx = ctx.household_tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        // Only forward messages for this household
                        let relevant = match &msg {
                            HouseholdWsMessage::TasksSync { household_id: hid, .. } => *hid == household_id,
                            HouseholdWsMessage::TaskCreated { task } => task.household_id == household_id,
                            HouseholdWsMessage::TaskUpdated { task } => task.household_id == household_id,
                            HouseholdWsMessage::TaskDeleted { household_id: hid, .. } => *hid == household_id,
                        };
                        if relevant {
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if socket.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            result = socket.recv() => {
                match result {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
