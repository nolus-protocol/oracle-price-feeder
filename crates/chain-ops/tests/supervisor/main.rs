use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::Result;
use tokio::{spawn, sync::Notify, time::timeout};
use tracing::Level;

use chain_ops::{
    service::{run, ShutdownResult},
    supervisor::{configuration::Configuration, Supervisor},
};

use self::builtin_tasks::{
    TestingBalanceReporter, TestingBroadcast, TestingProtocolWatcher,
};

mod application_defined;
mod builtin_tasks;

#[derive(Clone)]
struct Context {
    application_defined_tasks_count: Arc<AtomicUsize>,
    notify: Arc<Notify>,
}

#[tokio::test]
async fn supervisor() {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    let shutdown_result: ShutdownResult<Result<()>> =
        run(|task_spawner, task_result_rx| async move {
            let notify = Arc::new(Notify::new());

            let application_defined_tasks_count = Arc::new(AtomicUsize::new(0));

            let abort_handle = spawn(
                Supervisor::<
                    TestingBalanceReporter,
                    TestingBroadcast,
                    TestingProtocolWatcher,
                    application_defined::Task,
                >::new(
                    Configuration::new(
                        Context {
                            application_defined_tasks_count:
                                application_defined_tasks_count.clone(),
                            notify: notify.clone(),
                        },
                        (),
                    ),
                    task_spawner,
                    task_result_rx,
                    "supervisor-test",
                    "0.0.0",
                    [] as [application_defined::Id; 0],
                )
                .await?
                .run(),
            );

            () = timeout(Duration::from_secs(5), notify.notified())
                .await
                .unwrap();

            () = abort_handle.abort();

            _ = abort_handle.await.unwrap_err();

            assert_eq!(
                application_defined_tasks_count.load(Ordering::Acquire),
                0
            );

            Ok(())
        })
        .await
        .unwrap();

    () = match shutdown_result {
        ShutdownResult::Exited(join_result) => join_result.unwrap().unwrap(),
        ShutdownResult::StopSignalReceived => unreachable!(),
    };
}
