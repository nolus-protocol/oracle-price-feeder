use ::serde::{Deserialize, Serialize};

pub(super) mod serde;

#[derive(Serialize, Deserialize)]
#[must_use]
pub struct Currency {
    pub ticker: String,
    pub symbol: String,
}
