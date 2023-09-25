use std::io::{Result as IoResult, Write};

use tracing::{
    debug,
    dispatcher::{set_global_default, SetGlobalDefaultError},
    error, info, Dispatch,
};

use crate::{build_tx::TxResponse, interact::CommitResponse};

pub struct CombinedWriter<T, U>(T, U)
where
    T: Write + Send + Sync + 'static,
    U: Write + Send + Sync + 'static;

impl<T, U> CombinedWriter<T, U>
where
    T: Write + Send + Sync + 'static,
    U: Write + Send + Sync + 'static,
{
    pub fn new(first: T, second: U) -> Self {
        Self(first, second)
    }
}

impl<T, U> Write for CombinedWriter<T, U>
where
    T: Write + Send + Sync + 'static,
    U: Write + Send + Sync + 'static,
{
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        self.0.write(buf).and(self.1.write(buf))
    }

    fn flush(&mut self) -> IoResult<()> {
        self.0.flush().and(self.1.flush())
    }
}

pub fn setup<W>(writer: W) -> Result<(), SetGlobalDefaultError>
where
    W: for<'r> tracing_subscriber::fmt::MakeWriter<'r> + Send + Sync + 'static,
{
    set_global_default(Dispatch::new(
        tracing_subscriber::fmt()
            .with_level(true)
            .with_ansi(true)
            .with_file(false)
            .with_line_number(false)
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
            .finish(),
    ))
}

pub fn commit_response(response: &CommitResponse) {
    info!("Hash: {}", response.hash);

    for (tx_name, tx_result) in [
        ("Check", &response.check_tx as &dyn TxResponse),
        ("Tx", &response.deliver_tx as &dyn TxResponse),
    ] {
        {
            let (code, log) = (tx_result.code(), tx_result.log());

            if code.is_ok() {
                debug!("[{}] Log: {}", tx_name, log);
            } else {
                error!(
                    log = %log,
                    "[{}] Error with code {} has occurred!",
                    tx_name,
                    code.value(),
                );
            }
        }

        {
            let (gas_wanted, gas_used) = (tx_result.gas_wanted(), tx_result.gas_used());

            if gas_wanted < gas_used {
                error!(
                    wanted = %gas_wanted,
                    used = %gas_used,
                    "[{}] Out of gas!",
                    tx_name,
                );
            } else {
                info!(
                    "[{}] Gas used: {}; Gas limit for transacion: {}",
                    tx_name, gas_used, gas_wanted
                );
            }
        }
    }
}
