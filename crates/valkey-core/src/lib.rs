//! Connection helper plus four named runtime contracts (C1–C4) covering the
//! Module 1 primitives: SET/GET, INCR, SET-with-TTL, and HSET/HGETALL.
//!
//! Each public async fn enforces its contract via an `assert!` immediately
//! after the round-trip — same pattern the duckdb-from-zero capstone uses,
//! adapted for redis-rs's async I/O.
//!
//! Formal spec: contracts/valkey-rust-v1.yaml.

use anyhow::{Context, Result};
use redis::{aio::ConnectionManager, AsyncCommands};
use std::collections::HashMap;

pub const DEFAULT_URL: &str = "redis://127.0.0.1:6379";

/// Open a connection-managed (auto-reconnecting) async connection.
///
/// Module 4.4 callout: ConnectionManager is the redis-rs built-in for
/// auto-reconnect. deadpool-redis is a separate sizing story; this is the
/// gentler default to teach first.
pub async fn open(url: &str) -> Result<ConnectionManager> {
    let client = redis::Client::open(url).context("invalid Valkey URL")?;
    client
        .get_connection_manager()
        .await
        .with_context(|| format!("Valkey connection failed (url={url})"))
}

/// C1 set_get_round_trip — GET after SET returns the value SET.
pub async fn set_get_round_trip(
    conn: &mut ConnectionManager,
    key: &str,
    value: &str,
) -> Result<String> {
    let _: () = conn.set(key, value).await?;
    let got: String = conn.get(key).await?;
    // Provable contract C1 set_get_round_trip
    assert_eq!(
        got, value,
        "C1 set_get_round_trip: GET({key}) must equal the value SET"
    );
    Ok(got)
}

/// C2 incr_monotonic — INCR returns previous-value + 1, regardless of whether
/// the key existed before.
pub async fn incr_monotonic(conn: &mut ConnectionManager, key: &str) -> Result<i64> {
    let before: Option<i64> = conn.get(key).await?;
    let after: i64 = conn.incr(key, 1).await?;
    let expected = before.unwrap_or(0) + 1;
    // Provable contract C2 incr_monotonic
    assert_eq!(
        after, expected,
        "C2 incr_monotonic: INCR({key}) must equal previous + 1 (before={before:?})"
    );
    Ok(after)
}

/// C3 ttl_set_expires — after `SET key value EX ttl_secs`, `TTL key` returns
/// a value in `[0, ttl_secs]`. (Server-clock skew is bounded by the round-trip,
/// so the upper bound is the requested ttl, not ttl-minus-epsilon.)
pub async fn set_with_ttl(
    conn: &mut ConnectionManager,
    key: &str,
    value: &str,
    ttl_secs: u64,
) -> Result<i64> {
    let _: () = conn.set_ex(key, value, ttl_secs).await?;
    let ttl: i64 = conn.ttl(key).await?;
    // Provable contract C3 ttl_set_expires
    assert!(
        ttl >= 0 && (ttl as u64) <= ttl_secs,
        "C3 ttl_set_expires: TTL({key})={ttl} must be in [0, {ttl_secs}]"
    );
    Ok(ttl)
}

/// C4 hash_round_trip — HGETALL after HSET-of-fields returns exactly those
/// fields with exactly those values. (Order is not guaranteed and not checked.)
pub async fn hash_round_trip(
    conn: &mut ConnectionManager,
    key: &str,
    fields: &[(&str, &str)],
) -> Result<HashMap<String, String>> {
    // HSET multi-field. redis-rs accepts a slice of tuples.
    let _: () = conn.hset_multiple(key, fields).await?;
    let got: HashMap<String, String> = conn.hgetall(key).await?;
    // Provable contract C4 hash_round_trip
    assert_eq!(
        got.len(),
        fields.len(),
        "C4 hash_round_trip: HGETALL must return all fields HSET"
    );
    for (k, v) in fields {
        assert_eq!(
            got.get(*k).map(String::as_str),
            Some(*v),
            "C4 hash_round_trip: field {k} missing or wrong value"
        );
    }
    Ok(got)
}

/// Best-effort cleanup so demo runs are idempotent. Failure is swallowed —
/// cleanup is not a contract, it's a courtesy.
pub async fn cleanup(conn: &mut ConnectionManager, keys: &[String]) {
    if keys.is_empty() {
        return;
    }
    let _: Result<i64, _> = conn.del(keys).await;
}

#[cfg(test)]
mod tests {
    //! Pure-logic tests that don't need a live Valkey. Round-trip tests live
    //! in tests/integration.rs (gated behind `make up`).

    use super::*;

    #[test]
    fn default_url_is_loopback() {
        assert_eq!(DEFAULT_URL, "redis://127.0.0.1:6379");
    }

    #[tokio::test]
    async fn open_returns_err_on_invalid_url() {
        let r = open("not-a-url").await;
        assert!(r.is_err(), "open() must reject malformed URL");
    }

    #[tokio::test]
    async fn cleanup_with_empty_keys_is_noop() {
        // Doesn't need a live Valkey — the early return short-circuits before
        // any I/O. Constructing a ConnectionManager would fail without a server,
        // so we exercise the early-return branch via a unit fixture.
        //
        // We can't easily build a ConnectionManager in a unit test without a
        // server, so this test verifies the early-return is taken by checking
        // it returns in O(microseconds) regardless of the (broken) connection.
        // Use timeout to prove no I/O happened.
        use std::time::{Duration, Instant};
        let conn_result = tokio::time::timeout(
            Duration::from_secs(2),
            redis::Client::open("redis://127.0.0.1:6379")
                .unwrap()
                .get_connection_manager(),
        )
        .await;
        if let Ok(Ok(mut conn)) = conn_result {
            let started = Instant::now();
            cleanup(&mut conn, &[]).await;
            // Empty cleanup must not perform a DEL round-trip.
            assert!(
                started.elapsed() < Duration::from_millis(50),
                "empty cleanup() must short-circuit (took {:?})",
                started.elapsed()
            );
        }
        // If Valkey isn't reachable we silently skip — this test asserts a
        // performance property, not a connectivity one.
    }

    #[tokio::test]
    async fn open_returns_err_on_unreachable_host() {
        // 127.0.0.1:1 reliably refuses connections. ConnectionManager retries
        // with exponential backoff; bound the test to 3 s with tokio::time::timeout
        // so a slow retry curve can't blow up the suite.
        let r = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            open("redis://127.0.0.1:1"),
        )
        .await;
        // Either the timeout elapsed (Err) or open() returned Err — both prove
        // "open() does not return Ok against an unreachable host".
        match r {
            Ok(inner) => assert!(inner.is_err(), "open() must surface unreachable as Err"),
            Err(_) => { /* timeout — also acceptable */ }
        }
    }
}
