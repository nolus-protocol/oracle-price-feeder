use tracing::{debug, error, info, info_span};

use chain_comms::interact::commit::Response;

pub fn commit_response(response: &Response) {
    info_span!("Mempool Response").in_scope(|| {
        info!("Hash: {}", response.hash);

        if response.code.is_ok() {
            debug!("Log: {}", response.log);
        } else {
            error!(
                log = response.log,
                "Error with code {} has occurred!",
                response.code.value(),
            );
        }
    });
}
