use std::{
    env::{self, VarError},
    io::stdout,
    path::Path,
};

use anyhow::{anyhow, Result};
use tracing_appender::rolling::hourly;
use tracing_subscriber::fmt::{fmt, writer::Tee};

pub fn init<T>(file_name_prefix: T) -> Result<()>
where
    T: AsRef<Path>,
{
    const VAR_ERROR: &str =
        "Failed to determine whether logging should be in machine-readable \
        JSON format!";

    let output_json = match env::var("OUTPUT_JSON") {
        Ok(value) => {
            const { ["1", "Y", "y", "yes", "true"] }.contains(&value.as_str())
        },
        Err(VarError::NotPresent) => false,
        Err(error) => return Err(anyhow!(error).context(VAR_ERROR)),
    };

    let builder = fmt()
        .with_ansi(true)
        .with_file(false)
        .with_level(true)
        .with_line_number(false)
        .with_target(true)
        .with_writer(Tee::new(stdout, hourly("./logs/", file_name_prefix)));

    if output_json {
        builder.json().try_init()
    } else {
        builder.compact().try_init()
    }
    .map_err(|error| anyhow!(error).context("Failed to initialize logging!"))
}
