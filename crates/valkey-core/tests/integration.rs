//! Round-trip tests against a live Valkey instance. Run `make up` first.
//!
//! Each test uses a unique key prefix so the suite is parallel-safe and idempotent.

use valkey_core::{
    cleanup, hash_round_trip, incr_monotonic, open, set_get_round_trip, set_with_ttl, DEFAULT_URL,
};

fn url() -> String {
    std::env::var("VALKEY_URL").unwrap_or_else(|_| DEFAULT_URL.to_string())
}

fn key_prefix(test: &str) -> String {
    format!("vfz:test:{}:{test}:{}", std::process::id(), test_unique())
}

// Cheap monotonic-ish unique source so multiple #[tokio::test]s in the same
// process get distinct prefixes without pulling in `uuid` for the test build.
fn test_unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[tokio::test]
async fn integration_c1_set_get_round_trip() {
    let mut conn = open(&url()).await.expect("open");
    let key = format!("{}:k", key_prefix("c1"));
    let got = set_get_round_trip(&mut conn, &key, "hello, valkey")
        .await
        .expect("round-trip");
    assert_eq!(got, "hello, valkey");
    cleanup(&mut conn, &[key]).await;
}

#[tokio::test]
async fn integration_c2_incr_monotonic() {
    let mut conn = open(&url()).await.expect("open");
    let key = format!("{}:k", key_prefix("c2"));
    let a = incr_monotonic(&mut conn, &key).await.expect("incr1");
    let b = incr_monotonic(&mut conn, &key).await.expect("incr2");
    let c = incr_monotonic(&mut conn, &key).await.expect("incr3");
    assert_eq!((a, b, c), (1, 2, 3));
    cleanup(&mut conn, &[key]).await;
}

#[tokio::test]
async fn integration_c3_set_with_ttl() {
    let mut conn = open(&url()).await.expect("open");
    let key = format!("{}:k", key_prefix("c3"));
    let ttl = set_with_ttl(&mut conn, &key, "v", 60)
        .await
        .expect("set_ex");
    assert!((0..=60).contains(&ttl), "TTL {ttl} should be in [0, 60]");
    cleanup(&mut conn, &[key]).await;
}

#[tokio::test]
async fn integration_c4_hash_round_trip() {
    let mut conn = open(&url()).await.expect("open");
    let key = format!("{}:k", key_prefix("c4"));
    let got = hash_round_trip(
        &mut conn,
        &key,
        &[("first_name", "Alice"), ("last_name", "Liddell")],
    )
    .await
    .expect("hash");
    assert_eq!(got.get("first_name").map(String::as_str), Some("Alice"));
    assert_eq!(got.get("last_name").map(String::as_str), Some("Liddell"));
    cleanup(&mut conn, &[key]).await;
}
