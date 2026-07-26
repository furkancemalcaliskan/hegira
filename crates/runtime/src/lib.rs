use std::{future::Future, process::ExitCode};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRole {
    All,
    Web,
    Worker,
}

impl RuntimeRole {
    pub fn runs_web(&self) -> bool {
        matches!(self, Self::All | Self::Web)
    }

    pub fn runs_workers(&self) -> bool {
        matches!(self, Self::All | Self::Worker)
    }
}

pub fn run<Start, Process>(start: Start) -> ExitCode
where
    Start: FnOnce() -> Process,
    Process: Future<Output = Result<(), String>>,
{
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to initialize Tokio runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(start()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received; draining connections");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_roles_advertise_expected_processes() {
        assert!(RuntimeRole::All.runs_web());
        assert!(RuntimeRole::All.runs_workers());
        assert!(RuntimeRole::Web.runs_web());
        assert!(!RuntimeRole::Web.runs_workers());
        assert!(!RuntimeRole::Worker.runs_web());
        assert!(RuntimeRole::Worker.runs_workers());
    }

    #[test]
    fn process_runner_maps_success_and_failure_to_exit_codes() {
        assert_eq!(run(|| async { Ok(()) }), ExitCode::SUCCESS);
        assert_eq!(
            run(|| async { Err("expected failure".to_string()) }),
            ExitCode::FAILURE
        );
    }
}
