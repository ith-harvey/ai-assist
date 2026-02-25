//! Context manager for handling multiple job contexts.

use std::collections::HashMap;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::JobError;
use crate::worker::memory::Memory;
use crate::worker::state::{JobState, WorkerJobContext};

/// Manages contexts for multiple concurrent jobs.
pub struct ContextManager {
    /// Active job contexts.
    contexts: RwLock<HashMap<Uuid, WorkerJobContext>>,
    /// Memory for each job.
    memories: RwLock<HashMap<Uuid, Memory>>,
    /// Maximum concurrent jobs.
    max_jobs: usize,
}

impl ContextManager {
    /// Create a new context manager.
    pub fn new(max_jobs: usize) -> Self {
        Self {
            contexts: RwLock::new(HashMap::new()),
            memories: RwLock::new(HashMap::new()),
            max_jobs,
        }
    }

    /// Create a new job context.
    pub async fn create_job(
        &self,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Uuid, JobError> {
        self.create_job_for_user("default", title, description)
            .await
    }

    /// Create a new job context for a specific user.
    pub async fn create_job_for_user(
        &self,
        user_id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Uuid, JobError> {
        let mut contexts = self.contexts.write().await;
        let active_count = contexts.values().filter(|c| c.state.is_active()).count();

        if active_count >= self.max_jobs {
            return Err(JobError::MaxJobsExceeded { max: self.max_jobs });
        }

        let context = WorkerJobContext::with_user(user_id, title, description);
        let job_id = context.job_id;
        contexts.insert(job_id, context);
        drop(contexts);

        let memory = Memory::new(job_id);
        self.memories.write().await.insert(job_id, memory);

        Ok(job_id)
    }

    /// Create a job context from a pre-built WorkerJobContext.
    pub async fn register_job(&self, context: WorkerJobContext) -> Result<(), JobError> {
        let mut contexts = self.contexts.write().await;
        let active_count = contexts.values().filter(|c| c.state.is_active()).count();

        if active_count >= self.max_jobs {
            return Err(JobError::MaxJobsExceeded { max: self.max_jobs });
        }

        let job_id = context.job_id;
        contexts.insert(job_id, context);
        drop(contexts);

        let memory = Memory::new(job_id);
        self.memories.write().await.insert(job_id, memory);

        Ok(())
    }

    /// Get a job context by ID.
    pub async fn get_context(&self, job_id: Uuid) -> Result<WorkerJobContext, JobError> {
        self.contexts
            .read()
            .await
            .get(&job_id)
            .cloned()
            .ok_or(JobError::NotFound { id: job_id })
    }

    /// Get a mutable reference to update a job context.
    pub async fn update_context<F, R>(&self, job_id: Uuid, f: F) -> Result<R, JobError>
    where
        F: FnOnce(&mut WorkerJobContext) -> R,
    {
        let mut contexts = self.contexts.write().await;
        let context = contexts
            .get_mut(&job_id)
            .ok_or(JobError::NotFound { id: job_id })?;
        Ok(f(context))
    }

    /// Get job memory.
    pub async fn get_memory(&self, job_id: Uuid) -> Result<Memory, JobError> {
        self.memories
            .read()
            .await
            .get(&job_id)
            .cloned()
            .ok_or(JobError::NotFound { id: job_id })
    }

    /// Update job memory.
    pub async fn update_memory<F, R>(&self, job_id: Uuid, f: F) -> Result<R, JobError>
    where
        F: FnOnce(&mut Memory) -> R,
    {
        let mut memories = self.memories.write().await;
        let memory = memories
            .get_mut(&job_id)
            .ok_or(JobError::NotFound { id: job_id })?;
        Ok(f(memory))
    }

    /// List all active job IDs.
    pub async fn active_jobs(&self) -> Vec<Uuid> {
        self.contexts
            .read()
            .await
            .iter()
            .filter(|(_, c)| c.state.is_active())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get count of active jobs.
    pub async fn active_count(&self) -> usize {
        self.contexts
            .read()
            .await
            .values()
            .filter(|c| c.state.is_active())
            .count()
    }

    /// Remove a completed job (cleanup).
    pub async fn remove_job(&self, job_id: Uuid) -> Result<(WorkerJobContext, Memory), JobError> {
        let context = self
            .contexts
            .write()
            .await
            .remove(&job_id)
            .ok_or(JobError::NotFound { id: job_id })?;

        let memory = self
            .memories
            .write()
            .await
            .remove(&job_id)
            .ok_or(JobError::NotFound { id: job_id })?;

        Ok((context, memory))
    }

    /// Find stuck jobs.
    pub async fn find_stuck_jobs(&self) -> Vec<Uuid> {
        self.contexts
            .read()
            .await
            .iter()
            .filter(|(_, c)| c.state == JobState::Stuck)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get summary of all jobs.
    pub async fn summary(&self) -> ContextSummary {
        let contexts = self.contexts.read().await;

        let mut summary = ContextSummary::default();
        for ctx in contexts.values() {
            match ctx.state {
                JobState::Pending => summary.pending += 1,
                JobState::InProgress => summary.in_progress += 1,
                JobState::Completed => summary.completed += 1,
                JobState::Failed => summary.failed += 1,
                JobState::Stuck => summary.stuck += 1,
                JobState::Cancelled => summary.cancelled += 1,
            }
        }

        summary.total = contexts.len();
        summary
    }
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new(10)
    }
}

/// Summary of all job contexts.
#[derive(Debug, Default)]
pub struct ContextSummary {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub failed: usize,
    pub stuck: usize,
    pub cancelled: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_job() {
        let manager = ContextManager::new(5);
        let job_id = manager.create_job("Test", "Description").await.unwrap();

        let context = manager.get_context(job_id).await.unwrap();
        assert_eq!(context.title, "Test");
        assert_eq!(context.state, JobState::Pending);
    }

    #[tokio::test]
    async fn create_job_for_user() {
        let manager = ContextManager::new(5);
        let job_id = manager
            .create_job_for_user("user-123", "Test", "Description")
            .await
            .unwrap();

        let context = manager.get_context(job_id).await.unwrap();
        assert_eq!(context.user_id, "user-123");
    }

    #[tokio::test]
    async fn max_jobs_limit() {
        let manager = ContextManager::new(2);

        manager.create_job("Job 1", "Desc").await.unwrap();
        manager.create_job("Job 2", "Desc").await.unwrap();

        // Start the jobs to make them active
        for job_id in manager.active_jobs().await {
            manager
                .update_context(job_id, |ctx| {
                    ctx.transition_to(JobState::InProgress, None)
                })
                .await
                .unwrap()
                .unwrap();
        }

        // Third job should fail
        let result = manager.create_job("Job 3", "Desc").await;
        assert!(matches!(result, Err(JobError::MaxJobsExceeded { max: 2 })));
    }

    #[tokio::test]
    async fn update_context() {
        let manager = ContextManager::new(5);
        let job_id = manager.create_job("Test", "Desc").await.unwrap();

        manager
            .update_context(job_id, |ctx| {
                ctx.transition_to(JobState::InProgress, None)
            })
            .await
            .unwrap()
            .unwrap();

        let context = manager.get_context(job_id).await.unwrap();
        assert_eq!(context.state, JobState::InProgress);
    }

    #[tokio::test]
    async fn register_prebuilt_job() {
        let manager = ContextManager::new(5);
        let ctx = WorkerJobContext::new("Pre-built", "Already created");
        let job_id = ctx.job_id;
        manager.register_job(ctx).await.unwrap();

        let fetched = manager.get_context(job_id).await.unwrap();
        assert_eq!(fetched.title, "Pre-built");
    }

    #[tokio::test]
    async fn remove_job() {
        let manager = ContextManager::new(5);
        let job_id = manager.create_job("Test", "Desc").await.unwrap();

        let (ctx, mem) = manager.remove_job(job_id).await.unwrap();
        assert_eq!(ctx.title, "Test");
        assert_eq!(mem.job_id, job_id);

        // Should no longer be found
        assert!(manager.get_context(job_id).await.is_err());
    }

    #[tokio::test]
    async fn summary() {
        let manager = ContextManager::new(10);
        manager.create_job("Job 1", "Desc").await.unwrap();
        let job2 = manager.create_job("Job 2", "Desc").await.unwrap();

        manager
            .update_context(job2, |ctx| {
                ctx.transition_to(JobState::InProgress, None)
            })
            .await
            .unwrap()
            .unwrap();

        let summary = manager.summary().await;
        assert_eq!(summary.total, 2);
        assert_eq!(summary.pending, 1);
        assert_eq!(summary.in_progress, 1);
    }
}
