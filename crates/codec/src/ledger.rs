//! Process-wide diagnostic counts. One operation at a time; take only after workers join.
use std::{collections::BTreeMap, sync::Mutex};

static COUNTS: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());

pub fn add(site: &'static str, bytes: usize) {
    *COUNTS.lock().unwrap().entry(site).or_default() += bytes as u64;
}

pub fn take() -> BTreeMap<&'static str, u64> {
    std::mem::take(&mut *COUNTS.lock().unwrap())
}
