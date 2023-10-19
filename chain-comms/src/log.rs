use tracing::{debug, error, info};

use crate::{build_tx::TxResponse, interact::CommitResponse};

pub fn setup<W>(writer: W)
where
    W: for<'r> tracing_subscriber::fmt::MakeWriter<'r> + Send + Sync + 'static,
{
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
                    [OsStr::new("1"), OsStr::new("y"), OsStr::new("Y")].contains(&value.as_os_str())
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
