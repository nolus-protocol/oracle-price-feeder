use tracing_subscriber::fmt::format;

pub fn setup<W>(writer: W)
where
    W: for<'r> tracing_subscriber::fmt::MakeWriter<'r> + Send + Sync + 'static,
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
                tracing::level_filters::LevelFilter::DEBUG
            } else {
                tracing::level_filters::LevelFilter::INFO
            }
        })
        .init();
}
