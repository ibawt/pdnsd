use std::collections::HashMap;
use dns::*;
use time;

#[derive (Debug)]
struct Entry {
    msg: ResourceRecord,
    updated_at: u64
}

#[derive (Debug)]
pub struct Cache {
    entries: HashMap<String, Entry>
}

impl Cache {
    pub fn new() -> Cache {
        Cache{
            entries: HashMap::new()
        }
    }
}
