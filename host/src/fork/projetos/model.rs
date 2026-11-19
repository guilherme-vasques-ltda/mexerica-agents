use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct List {
    pub hub_id: String,
    pub space_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TasksPage {
    #[serde(rename = "hubId")]
    pub hub: String,
    #[serde(rename = "spaceId")]
    pub space: String,
    #[serde(rename = "afterProjectId")]
    pub after_project: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommentsRead {
    #[serde(rename = "hubId")]
    pub hub: String,
    #[serde(rename = "spaceId")]
    pub space: String,
    #[serde(rename = "projectId")]
    pub project: String,
    #[serde(rename = "taskId")]
    pub task: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommentWrite {
    pub hub_id: String,
    pub space_id: String,
    pub project_id: String,
    pub task_id: String,
    pub client_request_id: String,
    pub text: String,
    pub mentions: Vec<String>,
    pub only_note: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EnsureDefault {
    pub hub_id: String,
    pub space_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskMetaSave {
    pub hub_id: String,
    pub space_id: String,
    pub project_id: String,
    pub task_id: String,
    pub client_request_id: String,
    pub expected_revision: i64,
    pub meta: TaskMeta,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskMeta {
    pub priority: Option<Priority>,
    pub due_at: Option<i64>,
    pub phases: Vec<String>,
    pub current_phase: Option<usize>,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Priority {
    Low,
    Normal,
    High,
    Urgent,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Read {
    pub hub_id: String,
    pub space_id: String,
    #[serde(rename = "projectId")]
    pub project: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Create {
    pub hub_id: String,
    pub space_id: String,
    pub client_request_id: String,
    pub name: String,
    pub objective: String,
    pub coordinator_bot_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Save {
    pub hub_id: String,
    pub space_id: String,
    pub project_id: String,
    pub client_request_id: String,
    pub expected_revision: i64,
    pub name: String,
    pub objective: String,
    pub coordinator_bot_id: Option<String>,
    pub state: ProjectState,
    pub tasks: Vec<Task>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectState {
    Active,
    Archived,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskState {
    Backlog,
    InProgress,
    Review,
    Done,
    Blocked,
    Cancelled,
}
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Task {
    pub id: String,
    pub parent_task_id: Option<String>,
    pub assignee_bot_id: Option<String>,
    pub title: String,
    pub description: String,
    pub acceptance: String,
    pub evidence: String,
    pub state: TaskState,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Project {
    pub id: String,
    pub name: String,
    pub objective: String,
    pub coordinator_bot_id: Option<String>,
    pub state: ProjectState,
    #[serde(default)]
    pub is_default: bool,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}
