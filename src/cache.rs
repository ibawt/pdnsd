use std::collections::HashMap;
use dns::*;
use time;

#[derive (Debug)]
struct Entry {
    record: ResourceRecord,
    committed_at: f64
}

impl Entry {
    fn new(r: ResourceRecord) -> Entry {
        Entry {
            record: r,
            committed_at: time::precise_time_s()
        }
    }
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

    pub fn get(&self, key: &str) -> Option<&ResourceRecord> {
        self.entries.get(key).map(|entry| &entry.record)
    }

    pub fn set(&mut self, key: &str, record: &ResourceRecord) {
        self.entries.insert(key.to_owned(), Entry::new(record.clone()));
    }
}
