use std::{collections::BTreeSet, sync::Arc};

pub(super) struct StrPool(BTreeSet<Arc<str>>);

impl StrPool {
    pub const fn new() -> Self {
        Self(BTreeSet::new())
    }

    pub fn get_or_insert(&mut self, s: String) -> Arc<str> {
        if let Some(s) = self.0.get(s.as_str()) {
            s.clone()
        } else {
            let s: Arc<str> = Arc::from(s);

            #[cfg(debug_assertions)]
            let true: bool = self.0.insert(s.clone()) else {
                unreachable!()
            };
            #[cfg(not(debug_assertions))]
            self.0.insert(s.clone());

            s
        }
    }
}
