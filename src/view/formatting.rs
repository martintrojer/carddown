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

/// Format a set of tags as a sorted, comma-separated string.
///
/// Returns an empty string if the set is empty.
pub fn format_tags(tags: &HashSet<String>) -> String {
    if tags.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<_> = tags.iter().map(|s| s.as_str()).collect();
    sorted.sort();
    sorted.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tags_empty() {
        assert_eq!(format_tags(&HashSet::new()), "");
    }

    #[test]
    fn test_format_tags_single() {
        let tags = HashSet::from(["rust".to_string()]);
        assert_eq!(format_tags(&tags), "rust");
    }

    #[test]
    fn test_format_tags_multiple_sorted() {
        let tags = HashSet::from(["zebra".to_string(), "alpha".to_string(), "mid".to_string()]);
        assert_eq!(format_tags(&tags), "alpha, mid, zebra");
    }

    #[test]
    fn test_format_datetime_opt_none() {
        assert_eq!(format_datetime_opt(None, "never"), "never");
    }

    #[test]
    fn test_format_datetime_opt_some() {
        let dt = chrono::Utc::now();
        let result = format_datetime_opt(Some(dt), "never");
        assert_ne!(result, "never");
        assert!(result.contains('-')); // date format contains dashes
    }
}
