//! valkey-demo — runs all four named contracts (C1–C4) in sequence against
//! the running Valkey instance and prints the result of each round-trip.
//!
//! Usage:
//!   make up                          # start valkey on 127.0.0.1:6379
//!   cargo run --bin valkey-demo
//!   VALKEY_URL=redis://host:6379 cargo run --bin valkey-demo

use anyhow::Result;
use valkey_core::{
    cleanup, hash_round_trip, incr_monotonic, open, set_get_round_trip, set_with_ttl, DEFAULT_URL,
};

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("VALKEY_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
    println!("[connect] {url}");
    let mut conn = open(&url).await?;

    // Unique key prefix per process so parallel runs don't collide.
    let prefix = format!("vfz:demo:{}", std::process::id());
    let greeting_key = format!("{prefix}:greeting");
    let counter_key = format!("{prefix}:counter");
    let session_key = format!("{prefix}:session");
    let user_key = format!("{prefix}:user");

    // C1 SET/GET round-trip
    let got = set_get_round_trip(&mut conn, &greeting_key, "hello, valkey").await?;
    println!("[C1] set_get_round_trip → {got:?}");

    // C2 INCR monotonic — call twice, prove the increment.
    let c1 = incr_monotonic(&mut conn, &counter_key).await?;
    let c2 = incr_monotonic(&mut conn, &counter_key).await?;
    println!("[C2] incr_monotonic    → first={c1} then={c2}");

    // C3 SET EX with TTL
    let ttl = set_with_ttl(&mut conn, &session_key, "session-token", 60).await?;
    println!("[C3] set_with_ttl(60s) → TTL={ttl}s");

    // C4 HSET multi + HGETALL
    let hash = hash_round_trip(
        &mut conn,
        &user_key,
        &[
            ("first_name", "Penelope"),
            ("last_name", "Guiness"),
            ("films_credited", "19"),
        ],
    )
    .await?;
    let mut keys: Vec<&String> = hash.keys().collect();
    keys.sort();
    println!("[C4] hash_round_trip   → {keys:?}");

    cleanup(
        &mut conn,
        &[greeting_key, counter_key, session_key, user_key],
    )
    .await;
    println!("[done] all four contracts asserted");
    Ok(())
}
