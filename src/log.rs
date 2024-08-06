use std::{
    env::{self, VarError},
    fs::{create_dir, File},
    io::{stdout, Write},
    path::Path,
};

use anyhow::{anyhow, Result};
use chrono::{Datelike, Timelike, Utc};
use tracing_subscriber::fmt::{fmt, writer::MakeWriterExt, MakeWriter};

pub fn init<T>(logs_directory: T) -> Result<()>
where
    T: AsRef<Path>,
{
    fn monomorphic(logs_directory: &Path) -> Result<()> {
        const VAR_ERROR: &str =
            "Failed to determine whether logging should be in machine-readable \
            JSON format!";

        let output_json = match env::var("OUTPUT_JSON") {
            Ok(value) => const { ["1", "Y", "y", "yes", "true"] }
                .contains(&value.as_str()),
            Err(VarError::NotPresent) => false,
            Err(error) => return Err(anyhow!(error).context(VAR_ERROR)),
        };

        let builder = fmt()
            .with_ansi(true)
            .with_file(false)
            .with_level(true)
            .with_line_number(false)
            .with_target(true)
            .with_writer(stdout.and(DateTimeSegmentedWriterFactory::new(
                logs_directory.into(),
            )));

        if output_json {
            builder.json().try_init()
        } else {
            builder.compact().try_init()
        }
        .map_err(|error| {
            anyhow!(error).context("Failed to initialize logging!")
        })
    }

    monomorphic(logs_directory.as_ref())
}

struct DateTimeSegmentedWriterFactory {
    directory_path: Box<Path>,
}

impl DateTimeSegmentedWriterFactory {
    pub const fn new(directory_path: Box<Path>) -> Self {
        Self { directory_path }
    }
}

impl<'self_> MakeWriter<'self_> for DateTimeSegmentedWriterFactory {
    type Writer = DateTimeSegmentedWriter<'self_>;

    fn make_writer(&'self_ self) -> Self::Writer {
        DateTimeSegmentedWriter {
            directory_path: &self.directory_path,
            file: None,
        }
    }
}

struct DateTimeSegmentedWriter<'r> {
    directory_path: &'r Path,
    file: Option<(DateAndHour, File)>,
}

impl<'r> DateTimeSegmentedWriter<'r> {
    fn open_file(
        &mut self,
        date_and_hour: DateAndHour,
    ) -> std::io::Result<&mut File> {
        let mut file_path = self.directory_path.to_owned();

        [
            format!("year-{:0>2}", date_and_hour.year),
            format!("month-{:0>2}", date_and_hour.month),
            format!("day-{:0>2}", date_and_hour.day),
        ]
        .into_iter()
        .try_for_each(|segment| {
            file_path.push(segment);

            if file_path.exists() {
                Ok(())
            } else {
                create_dir(&*file_path)
            }
        })?;

        let file_path = {
            file_path.push(format!("hour-{:0>2}.log", date_and_hour.hour));

            file_path
        };

        File::options()
            .append(true)
            .create(true)
            .read(false)
            .open(file_path)
            .map(|file| &mut self.file.insert((date_and_hour, file)).1)
    }
}

impl<'r> Write for DateTimeSegmentedWriter<'r> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let now = DateAndHour::now();

        if let Some(file) = self
            .file
            .as_mut()
            .filter(|&&mut (date_and_hour, _)| date_and_hour == now)
            .map(|(_, file)| file)
        {
            file
        } else {
            self.file = None;

            self.open_file(now)?
        }
        .write(buf)
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        self.file
            .as_mut()
            .map_or(const { Ok(()) }, |(_, file)| file.flush())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DateAndHour {
    hour: u32,
    day: u32,
    month: u32,
    year: i32,
}

impl DateAndHour {
    pub fn now() -> Self {
        let utc = Utc::now().naive_utc();

        Self {
            hour: utc.hour(),
            day: utc.day(),
            month: utc.month(),
            year: utc.year(),
        }
    }
}
