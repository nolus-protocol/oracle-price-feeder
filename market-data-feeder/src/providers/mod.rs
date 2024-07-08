use self::{astroport::Astroport, osmosis::Osmosis};

pub mod astroport;
pub mod osmosis;

pub(crate) enum Provider {
    Astroport(Astroport),
    Osmosis(Osmosis),
}
