#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use chain_ops::run_app;

use market_data_feeder::task::{ApplicationDefinedContext, Id};

run_app!(
    task_creation_context: {
        ApplicationDefinedContext::new()
    },
    startup_tasks: [] as [Id; 0],
);
