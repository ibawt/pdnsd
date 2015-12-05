use std::collections::HashMap;
use dns::*;

#[derive (Debug)]
struct Entry {
    msg: Message,
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

    pub fn get(&self) -> Option<&Message> {
        None
    }

    pub fn set(&mut self, key: &str, age: u64) -> Option<()> {
        None
    }
}
