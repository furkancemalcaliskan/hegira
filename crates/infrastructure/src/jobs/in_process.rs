use application::shared::jobs::{Job, JobDispatcher};

#[derive(Debug, Clone, Copy)]
pub struct InProcessQueue;

impl JobDispatcher for InProcessQueue {
    fn dispatch<J, Args>(&self, job: J, args: Args)
    where
        J: Job<Args>,
        Args: Send + 'static,
    {
        tokio::spawn(async move {
            let name = job.name();
            match job.perform(args).await {
                Ok(()) => tracing::debug!(job = name, "job completed"),
                Err(error) => tracing::warn!(job = name, %error, "job failed"),
            }
        });
    }
}
