use std::fmt::Arguments;

use tracing::{debug, error, info, level_filters::LevelFilter};
use tracing_subscriber::fmt::{format, MakeWriter};

pub fn setup<W>(writer: W)
where
    W: for<'r> MakeWriter<'r> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .event_format(
            format()
                .with_ansi(true)
                .with_level(true)
                .with_target(false)
                .with_source_location(false)
                .with_file(false)
                .with_line_number(false)
                .compact(),
        )
        .with_writer(writer)
        .with_max_level({
            use std::{env::var_os, ffi::OsStr};

            if var_os("DEBUG_LOGGING")
                .map(|value| {
                    [OsStr::new("1"), OsStr::new("y"), OsStr::new("Y")]
                        .contains(&value.as_os_str())
                })
                .unwrap_or(cfg!(debug_assertions))
            {
                LevelFilter::DEBUG
            } else {
                LevelFilter::INFO
            }
        })
        .init();
}

pub fn debug_logs(formatted_arguments: &[Arguments<'_>]) {
    for formatted_arguments in formatted_arguments {
        debug!("{}", formatted_arguments);
    }
}

pub fn info_logs(formatted_arguments: &[Arguments<'_>]) {
    for formatted_arguments in formatted_arguments {
        info!("{}", formatted_arguments);
    }
}

pub fn error_logs(formatted_arguments: &[Arguments<'_>]) {
    for formatted_arguments in formatted_arguments {
        error!("{}", formatted_arguments);
    }
}
