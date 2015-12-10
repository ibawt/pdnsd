use std::collections::HashMap;
use dns::*;
use time;

#[derive (Debug)]
struct Entry {
    record: Vec<ResourceRecord>,
    committed_at: f64
}

impl Entry {
    fn new() -> Entry {
        Entry {
            record: vec![],
            committed_at: time::precise_time_s()
        }
    }

    fn records(&self) -> &[ResourceRecord] {
        &self.record
    }
}

#[derive (Debug)]
pub struct Cache{
    entries: HashMap<String, Entry>
}

impl Cache {
    pub fn new() -> Cache {
        Cache{
            entries: HashMap::new()
        }
    }

    pub fn get(&self, key: &str) -> Option<&[ResourceRecord]> {
        self.entries.get(key).map(|entry| entry.records())
    }

    pub fn add(&mut self, key: &str, rec: ResourceRecord) {
        let mut entry = self.entries.entry(key.to_owned()).or_insert(Entry::new());

        entry.record.push(rec);
    }
}
