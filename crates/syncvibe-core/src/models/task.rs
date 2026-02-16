use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::InProgress => write!(f, "in_progress"),
            TaskStatus::Completed => write!(f, "completed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub assigned_to: Option<String>,
    pub assigned_name: Option<String>,
    pub created_by: String,
    pub created_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Task {
    pub fn new(title: String, created_by: String, created_name: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            description: None,
            status: TaskStatus::Pending,
            assigned_to: None,
            assigned_name: None,
            created_by,
            created_name,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskBoard {
    pub tasks: Vec<Task>,
    pub version: u64,
}

impl Default for TaskBoard {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            version: 0,
        }
    }
}

impl TaskBoard {
    pub fn add_task(&mut self, task: Task) {
        self.tasks.push(task);
        self.version += 1;
    }

    pub fn find_task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn find_task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn remove_task(&mut self, id: &str) -> bool {
        let len = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        if self.tasks.len() < len {
            self.version += 1;
            true
        } else {
            false
        }
    }

    pub fn tasks_by_status(&self, status: &TaskStatus) -> Vec<&Task> {
        self.tasks.iter().filter(|t| &t.status == status).collect()
    }
}
