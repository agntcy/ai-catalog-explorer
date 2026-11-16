//! The catalogs the explorer knows about, kept in memory and saved to a JSON file.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MAX_ITEMS: usize = 100;
pub const MAX_RECORDS: usize = 500;

#[derive(Clone, Serialize, Deserialize)]
pub struct Item {
    pub identifier: String,
    pub name: Option<String>,
    pub entry_type: String,
    pub endorsed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Record {
    /// What was asked for: a domain or a catalog address.
    pub input: String,
    pub catalog_url: Option<String>,
    pub method: Option<String>,
    /// `ok` or `unreachable`.
    pub status: String,
    pub reason: Option<String>,
    pub level: Option<String>,
    pub valid: bool,
    pub host: Option<String>,
    pub items: Vec<Item>,
    /// Seconds since the Unix epoch.
    pub checked: u64,
}

impl Record {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.trim().to_string(),
            catalog_url: None,
            method: None,
            status: "unreachable".into(),
            reason: None,
            level: None,
            valid: false,
            host: None,
            items: Vec::new(),
            checked: now(),
        }
    }

    /// `trusted`, `discoverable`, `minimal`, `invalid` or `unreachable`.
    fn rating(&self) -> &str {
        if self.status != "ok" {
            "unreachable"
        } else if !self.valid {
            "invalid"
        } else {
            self.level.as_deref().unwrap_or("unknown")
        }
    }

    fn is(&self, other: &Record) -> bool {
        self.input.eq_ignore_ascii_case(&other.input)
            || (self.catalog_url.is_some() && self.catalog_url == other.catalog_url)
    }
}

#[derive(Default, Deserialize)]
pub struct Query {
    #[serde(default)]
    pub q: String,
    #[serde(default, rename = "type")]
    pub entry_type: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub method: String,
}

#[derive(Serialize)]
pub struct Stats {
    catalogs: usize,
    entries: usize,
    endorsed: usize,
    levels: BTreeMap<String, usize>,
    types: Vec<(String, usize)>,
    methods: BTreeMap<String, usize>,
    last_checked: u64,
}

#[derive(Serialize)]
pub struct Hit {
    #[serde(flatten)]
    record: Record,
    rating: String,
}

#[derive(Serialize)]
pub struct Answer {
    stats: Stats,
    records: Vec<Hit>,
}

pub struct Index {
    path: PathBuf,
    records: Mutex<Vec<Record>>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Index {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let records = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            records: Mutex::new(records),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Record>> {
        self.records.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn inputs(&self) -> Vec<String> {
        self.lock().iter().map(|r| r.input.clone()).collect()
    }

    /// When the input was last checked, and whether a catalog was found.
    pub fn checked(&self, input: &str) -> Option<(u64, bool)> {
        self.lock()
            .iter()
            .find(|r| r.input.eq_ignore_ascii_case(input))
            .map(|r| (r.checked, r.status == "ok"))
    }

    pub fn contains(&self, record: &Record) -> bool {
        self.lock().iter().any(|r| r.is(record))
    }

    /// Adds the record or replaces the one for the same catalog, then saves.
    pub fn upsert(&self, mut record: Record) {
        record.items.truncate(MAX_ITEMS);
        let mut records = self.lock();
        match records.iter().position(|r| r.is(&record)) {
            Some(i) => records[i] = record,
            None => records.push(record),
        }
        self.save(&records);
    }

    fn save(&self, records: &[Record]) {
        if self.path.as_os_str().is_empty() {
            return;
        }
        let temp = self.path.with_extension("tmp");
        let written = serde_json::to_vec_pretty(records)
            .map_err(std::io::Error::other)
            .and_then(|bytes| std::fs::write(&temp, bytes))
            .and_then(|()| std::fs::rename(&temp, &self.path));
        if let Err(error) = written {
            eprintln!("cannot save {}: {error}", self.path.display());
        }
    }

    pub fn query(&self, query: &Query) -> Answer {
        let records = self.lock();
        let q = query.q.trim().to_lowercase();
        let has = |text: &str| text.to_lowercase().contains(&q);
        let mut hits = Vec::new();
        for r in records.iter() {
            let rating = r.rating();
            let wanted = |filter: &str, value: Option<&str>| {
                filter.is_empty() || value.is_some_and(|v| v == filter)
            };
            if !wanted(&query.level, Some(rating)) || !wanted(&query.method, r.method.as_deref()) {
                continue;
            }
            let record_hit = q.is_empty()
                || has(&r.input)
                || r.catalog_url.as_deref().is_some_and(has)
                || r.host.as_deref().is_some_and(has);
            let items: Vec<Item> = r
                .items
                .iter()
                .filter(|i| query.entry_type.is_empty() || i.entry_type == query.entry_type)
                .filter(|i| record_hit || has(&i.identifier) || i.name.as_deref().is_some_and(has))
                .cloned()
                .collect();
            if (query.entry_type.is_empty() || !items.is_empty())
                && (record_hit || !items.is_empty())
            {
                hits.push(Hit {
                    record: Record { items, ..r.clone() },
                    rating: rating.to_string(),
                });
            }
        }
        hits.sort_by(|a, b| {
            rank(&a.rating)
                .cmp(&rank(&b.rating))
                .then_with(|| b.record.items.len().cmp(&a.record.items.len()))
                .then_with(|| a.record.input.cmp(&b.record.input))
        });
        Answer {
            stats: stats(&records),
            records: hits,
        }
    }
}

fn rank(rating: &str) -> u8 {
    match rating {
        "trusted" => 0,
        "discoverable" => 1,
        "minimal" => 2,
        "invalid" => 3,
        _ => 4,
    }
}

fn stats(records: &[Record]) -> Stats {
    let mut levels = BTreeMap::new();
    let mut types: BTreeMap<String, usize> = BTreeMap::new();
    let mut methods = BTreeMap::new();
    let (mut entries, mut endorsed) = (0, 0);
    for r in records {
        *levels.entry(r.rating().to_string()).or_insert(0) += 1;
        if let Some(method) = &r.method {
            *methods.entry(method.clone()).or_insert(0) += 1;
        }
        for item in &r.items {
            entries += 1;
            endorsed += usize::from(item.endorsed);
            *types.entry(item.entry_type.clone()).or_insert(0) += 1;
        }
    }
    let mut types: Vec<_> = types.into_iter().collect();
    types.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Stats {
        catalogs: records.len(),
        entries,
        endorsed,
        levels,
        types,
        methods,
        last_checked: records.iter().map(|r| r.checked).max().unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(input: &str, level: &str, items: &[(&str, &str)]) -> Record {
        Record {
            status: "ok".into(),
            valid: true,
            level: Some(level.into()),
            method: Some("well-known".into()),
            items: items
                .iter()
                .map(|(id, t)| Item {
                    identifier: (*id).into(),
                    name: None,
                    entry_type: (*t).into(),
                    endorsed: false,
                })
                .collect(),
            ..Record::new(input)
        }
    }

    fn index() -> Index {
        let index = Index::load("");
        index.upsert(record(
            "a.example",
            "trusted",
            &[("urn:a:mcp:x", "mcp"), ("urn:a:a2a:y", "a2a")],
        ));
        index.upsert(record("b.example", "minimal", &[("urn:b:mcp:z", "mcp")]));
        index
    }

    #[test]
    fn filters_by_type_and_level_and_narrows_the_entries() {
        let index = index();
        let answer = index.query(&Query {
            entry_type: "mcp".into(),
            level: "trusted".into(),
            ..Query::default()
        });
        assert_eq!(answer.records.len(), 1);
        assert_eq!(answer.records[0].record.items.len(), 1);
        assert_eq!(answer.stats.catalogs, 2);
    }

    #[test]
    fn text_matches_a_catalog_or_one_of_its_entries() {
        let index = index();
        let by_domain = index.query(&Query {
            q: "B.EXAMPLE".into(),
            ..Query::default()
        });
        assert_eq!(by_domain.records.len(), 1);
        let by_entry = index.query(&Query {
            q: "a2a:y".into(),
            ..Query::default()
        });
        assert_eq!(by_entry.records.len(), 1);
        assert_eq!(by_entry.records[0].record.items.len(), 1);
    }

    #[test]
    fn a_new_check_replaces_the_old_record() {
        let index = index();
        index.upsert(record("a.example", "minimal", &[]));
        assert_eq!(index.len(), 2);
        assert_eq!(
            index.query(&Query::default()).stats.levels.get("trusted"),
            None
        );
    }
}
