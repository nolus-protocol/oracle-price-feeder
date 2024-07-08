#[macro_export]
macro_rules! init_log {
    () => {
        () = $crate::log::init(env!("CARGO_PKG_NAME"))?;
    };
}

#[macro_export]
macro_rules! run_app {
    (
        task_creation_context: $task_creation_context:expr,
        startup_tasks: $startup_tasks:expr $(,)?
    ) => {
        #[::tokio::main]
        async fn main() -> ::anyhow::Result<()> {
            $crate::run::run(
                ::core::env!("CARGO_PKG_NAME"),
                ::core::env!("CARGO_PKG_VERSION"),
                || $task_creation_context,
                || $startup_tasks,
            )
            .await
        }
    };
}
