#[macro_export]
macro_rules! run_app {
    (
        task_creation_context: $task_creation_context:expr,
        startup_tasks: $startup_tasks:expr $(,)?
    ) => {
        #[::tokio::main]
        async fn main() -> ::anyhow::Result<()> {
            $crate::run::run::<String, _, _, _>(
                ::core::env!("CARGO_PKG_NAME"),
                ::core::env!("CARGO_PKG_VERSION"),
                ::anyhow::Context::context(
                    $crate::env::ReadFromVar::read_from_var("LOGS_DIRECTORY"),
                    "Failed to fetch log storing directory!",
                )?,
                || $task_creation_context,
                || $startup_tasks,
            )
            .await
        }
    };
}
