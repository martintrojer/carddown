use chrono::{DateTime, Local};
use std::collections::HashSet;

/// Format a DateTime as a string in local time
pub fn format_datetime(dt: DateTime<chrono::Utc>) -> String {
    let l: DateTime<Local> = DateTime::from(dt);
    l.format("%Y-%m-%d %H:%M").to_string()
}

/// Format an optional DateTime as a string, with a fallback for None
pub fn format_datetime_opt(dt: Option<DateTime<chrono::Utc>>, fallback: &str) -> String {
    dt.map(format_datetime)
        .unwrap_or_else(|| fallback.to_string())
}

/// Format a set of tags as a comma-separated string.
///
/// Returns an empty string if the set is empty.
pub fn format_tags(tags: &HashSet<String>) -> String {
    if tags.is_empty() {
        return String::new();
    }
    tags.iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

