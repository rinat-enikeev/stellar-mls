use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde_json::Value;

/// A subscription record stored in the database.
#[derive(Clone)]
pub struct Subscription {
    pub subscription_id: String,
    pub filter_json: String,
    pub encrypted_token: Vec<u8>,
    pub encrypted_notif_key: Vec<u8>,
    pub platform: String,
    pub created_at: i64,
    pub last_pushed_at: i64,
}

/// SQLite database for push notification subscriptions.
/// Uses a Mutex to make the rusqlite Connection thread-safe.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open (or create) the subscriptions database in the given data directory.
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| format!("failed to create data directory: {e}"))?;

        let db_path = data_dir.join("subscriptions.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("failed to open database: {e}"))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| format!("failed to set PRAGMA: {e}"))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS subscriptions (
                subscription_id TEXT PRIMARY KEY,
                filter_json TEXT NOT NULL,
                encrypted_token BLOB NOT NULL,
                encrypted_notif_key BLOB NOT NULL,
                platform TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_pushed_at INTEGER DEFAULT 0
            )",
            [],
        )
        .map_err(|e| format!("failed to create table: {e}"))?;

        eprintln!("Database initialized at {}", db_path.display());
        Ok(Database {
            conn: Mutex::new(conn),
        })
    }

    /// Insert or replace a subscription (upsert).
    pub fn insert_subscription(&self, sub: &Subscription) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("db lock error: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO subscriptions
             (subscription_id, filter_json, encrypted_token, encrypted_notif_key, platform, created_at, last_pushed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                sub.subscription_id,
                sub.filter_json,
                sub.encrypted_token,
                sub.encrypted_notif_key,
                sub.platform,
                sub.created_at,
                sub.last_pushed_at,
            ],
        )
        .map_err(|e| format!("failed to insert subscription: {e}"))?;
        Ok(())
    }

    /// Update only the filter for an existing subscription.
    pub fn update_subscription_filter(
        &self,
        subscription_id: &str,
        filter_json: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("db lock error: {e}"))?;
        let changed = conn
            .execute(
                "UPDATE subscriptions SET filter_json = ?1 WHERE subscription_id = ?2",
                params![filter_json, subscription_id],
            )
            .map_err(|e| format!("failed to update subscription filter: {e}"))?;
        if changed == 0 {
            return Err(format!("subscription not found: {subscription_id}"));
        }
        Ok(())
    }

    /// Delete a subscription by ID.
    pub fn delete_subscription(&self, subscription_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("db lock error: {e}"))?;
        conn.execute(
            "DELETE FROM subscriptions WHERE subscription_id = ?1",
            params![subscription_id],
        )
        .map_err(|e| format!("failed to delete subscription: {e}"))?;
        Ok(())
    }

    /// Find all subscriptions whose filters contain the given tag value.
    pub fn get_subscriptions_for_tag(&self, tag_value: &str) -> Result<Vec<Subscription>, String> {
        let conn = self.conn.lock().map_err(|e| format!("db lock error: {e}"))?;
        // We search for the tag value appearing anywhere in filter_json.
        // Since filters store tags as JSON arrays like "#t":["abc123"], we search
        // for the tag value as a JSON string within the filter.
        //
        // SECURITY: Escape SQL LIKE wildcards (% and _) in the tag value to prevent
        // attacker-controlled Nostr event tags from matching unintended subscriptions.
        // Without escaping, a tag value of "%" would match ALL subscriptions.
        let escaped = tag_value
            .replace('\\', "\\\\")  // Escape backslash first
            .replace('%', "\\%")    // Escape % wildcard
            .replace('_', "\\_")    // Escape _ wildcard
            .replace('"', "");      // Strip quotes (for JSON string matching)
        let pattern = format!("%\"{}\"%", escaped);

        let mut stmt = conn
            .prepare(
                "SELECT subscription_id, filter_json, encrypted_token, encrypted_notif_key,
                        platform, created_at, last_pushed_at
                 FROM subscriptions WHERE filter_json LIKE ?1 ESCAPE '\\'",
            )
            .map_err(|e| format!("failed to prepare query: {e}"))?;

        let rows = stmt
            .query_map(params![pattern], |row| {
                Ok(Subscription {
                    subscription_id: row.get(0)?,
                    filter_json: row.get(1)?,
                    encrypted_token: row.get(2)?,
                    encrypted_notif_key: row.get(3)?,
                    platform: row.get(4)?,
                    created_at: row.get(5)?,
                    last_pushed_at: row.get(6)?,
                })
            })
            .map_err(|e| format!("failed to query subscriptions: {e}"))?;

        let mut subs = Vec::new();
        for row in rows {
            subs.push(row.map_err(|e| format!("failed to read row: {e}"))?);
        }
        Ok(subs)
    }

    /// Get all unique tag values from all subscription filters.
    pub fn get_all_tags(&self) -> Result<Vec<String>, String> {
        let conn = self.conn.lock().map_err(|e| format!("db lock error: {e}"))?;
        let mut stmt = conn
            .prepare("SELECT filter_json FROM subscriptions")
            .map_err(|e| format!("failed to prepare query: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                let json_str: String = row.get(0)?;
                Ok(json_str)
            })
            .map_err(|e| format!("failed to query filters: {e}"))?;

        let mut all_tags = std::collections::HashSet::new();

        for row in rows {
            let json_str = row.map_err(|e| format!("failed to read row: {e}"))?;
            if let Ok(filter) = serde_json::from_str::<Value>(&json_str) {
                extract_tag_values(&filter, &mut all_tags);
            }
        }

        Ok(all_tags.into_iter().collect())
    }

    /// Update the last_pushed_at timestamp for a subscription.
    pub fn update_last_pushed(
        &self,
        subscription_id: &str,
        timestamp: i64,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("db lock error: {e}"))?;
        conn.execute(
            "UPDATE subscriptions SET last_pushed_at = ?1 WHERE subscription_id = ?2",
            params![timestamp, subscription_id],
        )
        .map_err(|e| format!("failed to update last_pushed_at: {e}"))?;
        Ok(())
    }

    /// Delete subscriptions older than max_age_days (based on both created_at and last_pushed_at).
    pub fn delete_stale_subscriptions(&self, max_age_days: i64) -> Result<u64, String> {
        let conn = self.conn.lock().map_err(|e| format!("db lock error: {e}"))?;
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
            - (max_age_days * 86400);

        let deleted = conn
            .execute(
                "DELETE FROM subscriptions WHERE last_pushed_at < ?1 AND created_at < ?1",
                params![cutoff],
            )
            .map_err(|e| format!("failed to delete stale subscriptions: {e}"))?;
        Ok(deleted as u64)
    }

    /// Get the last_pushed_at timestamp for a subscription.
    pub fn get_last_pushed(&self, subscription_id: &str) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| format!("db lock error: {e}"))?;
        conn.query_row(
            "SELECT last_pushed_at FROM subscriptions WHERE subscription_id = ?1",
            params![subscription_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("failed to get last_pushed_at: {e}"))
    }
}

/// Extract ALL tag values from any `#xxx` field in a Nostr filter.
/// Filters look like: {"kinds":[44114],"#t":["abc123"],"#sep_inbox":["def456"]}
pub fn extract_tag_values(
    filter: &Value,
    tags: &mut std::collections::HashSet<String>,
) {
    if let Some(obj) = filter.as_object() {
        for (key, value) in obj {
            if key.starts_with('#') {
                if let Some(arr) = value.as_array() {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            tags.insert(s.to_string());
                        }
                    }
                }
            }
        }
    }
}

/// Extract tag values that belong to a specific tag name (e.g. "#t" or "#sep_inbox").
/// Returns the tag name (without #) and the values.
pub fn extract_tags_by_name(filter: &Value) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    if let Some(obj) = filter.as_object() {
        for (key, value) in obj {
            if key.starts_with('#') {
                let tag_name = key[1..].to_string();
                if let Some(arr) = value.as_array() {
                    let values: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    if !values.is_empty() {
                        result.push((tag_name, values));
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = PathBuf::from(format!(
            "/tmp/pn_relay_db_test_{}_{}_{id}",
            std::process::id(),
            prefix
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_subscription(id: &str, filter: &str, platform: &str) -> Subscription {
        Subscription {
            subscription_id: id.to_string(),
            filter_json: filter.to_string(),
            encrypted_token: vec![1, 2, 3],
            encrypted_notif_key: vec![4, 5, 6],
            platform: platform.to_string(),
            created_at: 1000,
            last_pushed_at: 0,
        }
    }

    #[test]
    fn test_database_creation() {
        let dir = unique_temp_dir("db_create");
        let result = Database::new(&dir);
        assert!(result.is_ok());
        assert!(dir.join("subscriptions.db").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_insert_subscription() {
        let dir = unique_temp_dir("db_insert");
        let db = Database::new(&dir).unwrap();

        let sub = make_subscription("test-sub-1", r##"{"#t":["abc123"]}"##, "ios");
        db.insert_subscription(&sub).unwrap();

        // Retrieve via tag query
        let results = db.get_subscriptions_for_tag("abc123").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].subscription_id, "test-sub-1");
        assert_eq!(results[0].platform, "ios");
        assert_eq!(results[0].encrypted_token, vec![1, 2, 3]);
        assert_eq!(results[0].encrypted_notif_key, vec![4, 5, 6]);
        assert_eq!(results[0].created_at, 1000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_insert_subscription_upsert() {
        let dir = unique_temp_dir("db_upsert");
        let db = Database::new(&dir).unwrap();

        let sub1 = make_subscription("test-sub-1", r##"{"#t":["abc123"]}"##, "ios");
        db.insert_subscription(&sub1).unwrap();

        // Insert same ID with different data
        let sub2 = Subscription {
            subscription_id: "test-sub-1".to_string(),
            filter_json: r##"{"#t":["xyz789"]}"##.to_string(),
            encrypted_token: vec![10, 20, 30],
            encrypted_notif_key: vec![40, 50, 60],
            platform: "android-fcm".to_string(),
            created_at: 2000,
            last_pushed_at: 500,
        };
        db.insert_subscription(&sub2).unwrap();

        // Old tag should return no results
        let old = db.get_subscriptions_for_tag("abc123").unwrap();
        assert_eq!(old.len(), 0);

        // New tag should return the updated subscription
        let new = db.get_subscriptions_for_tag("xyz789").unwrap();
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].platform, "android-fcm");
        assert_eq!(new[0].encrypted_token, vec![10, 20, 30]);
        assert_eq!(new[0].created_at, 2000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_update_subscription_filter() {
        let dir = unique_temp_dir("db_update_filter");
        let db = Database::new(&dir).unwrap();

        let sub = make_subscription("test-sub-1", r##"{"#t":["old_tag"]}"##, "ios");
        db.insert_subscription(&sub).unwrap();

        db.update_subscription_filter("test-sub-1", r##"{"#t":["new_tag"]}"##)
            .unwrap();

        // Old tag should not match
        let old = db.get_subscriptions_for_tag("old_tag").unwrap();
        assert_eq!(old.len(), 0);

        // New tag should match
        let new = db.get_subscriptions_for_tag("new_tag").unwrap();
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].subscription_id, "test-sub-1");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_update_subscription_not_found() {
        let dir = unique_temp_dir("db_update_notfound");
        let db = Database::new(&dir).unwrap();

        let result = db.update_subscription_filter("nonexistent", r##"{"#t":["tag"]}"##);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_subscription() {
        let dir = unique_temp_dir("db_delete");
        let db = Database::new(&dir).unwrap();

        let sub = make_subscription("test-sub-1", r##"{"#t":["abc123"]}"##, "ios");
        db.insert_subscription(&sub).unwrap();

        db.delete_subscription("test-sub-1").unwrap();

        let results = db.get_subscriptions_for_tag("abc123").unwrap();
        assert_eq!(results.len(), 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_subscription_not_found() {
        let dir = unique_temp_dir("db_delete_notfound");
        let db = Database::new(&dir).unwrap();

        // Deleting a non-existent subscription should not error
        let result = db.delete_subscription("nonexistent");
        assert!(result.is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_subscriptions_by_tag() {
        let dir = unique_temp_dir("db_by_tag");
        let db = Database::new(&dir).unwrap();

        let sub1 = make_subscription("sub-1", r##"{"#t":["shared_tag","unique_1"]}"##, "ios");
        let sub2 = make_subscription("sub-2", r##"{"#t":["shared_tag","unique_2"]}"##, "android-fcm");
        let sub3 = make_subscription("sub-3", r##"{"#t":["other_tag"]}"##, "ios");

        db.insert_subscription(&sub1).unwrap();
        db.insert_subscription(&sub2).unwrap();
        db.insert_subscription(&sub3).unwrap();

        // Query shared tag — should return sub-1 and sub-2
        let shared = db.get_subscriptions_for_tag("shared_tag").unwrap();
        assert_eq!(shared.len(), 2);
        let ids: Vec<&str> = shared.iter().map(|s| s.subscription_id.as_str()).collect();
        assert!(ids.contains(&"sub-1"));
        assert!(ids.contains(&"sub-2"));

        // Query unique tag — should return only one
        let unique = db.get_subscriptions_for_tag("unique_1").unwrap();
        assert_eq!(unique.len(), 1);
        assert_eq!(unique[0].subscription_id, "sub-1");

        // Query tag with no matches
        let none = db.get_subscriptions_for_tag("no_such_tag").unwrap();
        assert_eq!(none.len(), 0);

        std::fs::remove_dir_all(&dir).ok();
    }
}
