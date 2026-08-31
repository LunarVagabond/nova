//! Built-in dynamic placeholders — `{{$uuid}}`, `{{$timestamp}}`, and
//! friends — computed fresh every time [`resolve`](crate::ParsedRequest::resolve)
//! runs, rather than looked up in an environment, collection, or
//! chained-variable map.
//!
//! A dynamic placeholder is recognized syntactically: its name starts with
//! `$`, which no ordinary variable name ever does (environment/collection
//! variables come from YAML keys, and `[assert]` extraction names come from
//! hand-written `<name> = <term>` lines — nothing in either path produces a
//! leading `$`). That's enough for [`resolve`] to tell the two apart
//! without needing a separate placeholder syntax.

use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

/// If `name` (the trimmed text between `{{` and `}}`, e.g. `$uuid`) names a
/// built-in dynamic placeholder, compute and return its value.
///
/// Returns `None` for a name that doesn't start with `$` (an ordinary
/// variable, left to the environment lookup) and also for a `$name` this
/// crate doesn't recognize — the caller reports that as an undefined
/// variable, the same as any other unresolved placeholder.
pub(crate) fn resolve(name: &str) -> Option<String> {
    Some(match name {
        "$uuid" => Uuid::new_v4().to_string(),
        "$timestamp" => unix_timestamp().to_string(),
        "$randomInt" => rand::random_range(0..=1000_i64).to_string(),
        "$randomEmail" => random_email(),
        _ => return None,
    })
}

/// Seconds since the Unix epoch, as a decimal string. Clamped to `0` in the
/// (practically impossible) case the system clock reads before 1970.
fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// A throwaway `local@example.com` address — a random 10-character
/// lowercase-alphanumeric local part at the reserved `example.com` domain,
/// so a generated address never resolves to a real inbox.
fn random_email() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let local: String = (0..10)
        .map(|_| CHARS[rand::random_range(0..CHARS.len())] as char)
        .collect();
    format!("{local}@example.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_not_dynamic() {
        assert!(resolve("base_url").is_none());
        assert!(resolve("token").is_none());
    }

    #[test]
    fn unrecognized_dollar_names_are_not_dynamic() {
        assert!(resolve("$nope").is_none());
    }

    #[test]
    fn uuid_has_uuid_shape() {
        let value = resolve("$uuid").expect("$uuid should resolve");
        let parsed = Uuid::parse_str(&value).expect("should be a valid UUID");
        assert_eq!(parsed.get_version_num(), 4);
    }

    #[test]
    fn uuid_is_fresh_each_call() {
        let first = resolve("$uuid").unwrap();
        let second = resolve("$uuid").unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn timestamp_is_a_plausible_epoch_value() {
        let value = resolve("$timestamp").expect("$timestamp should resolve");
        let parsed: u64 = value.parse().expect("should be a decimal integer");
        // 2020-01-01T00:00:00Z, as a sanity floor — comfortably below "now"
        // for any reasonable clock, and rules out an accidental
        // milliseconds-since-epoch or all-zero value.
        assert!(parsed > 1_577_836_800);
    }

    #[test]
    fn random_int_is_in_range() {
        for _ in 0..50 {
            let value = resolve("$randomInt").expect("$randomInt should resolve");
            let parsed: i64 = value.parse().expect("should be a decimal integer");
            assert!((0..=1000).contains(&parsed));
        }
    }

    #[test]
    fn random_email_has_email_shape() {
        let value = resolve("$randomEmail").expect("$randomEmail should resolve");
        let (local, domain) = value.split_once('@').expect("should contain exactly one @");
        assert_eq!(local.len(), 10);
        assert!(local
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert_eq!(domain, "example.com");
    }

    #[test]
    fn random_email_is_fresh_each_call() {
        let first = resolve("$randomEmail").unwrap();
        let second = resolve("$randomEmail").unwrap();
        assert_ne!(first, second);
    }
}
