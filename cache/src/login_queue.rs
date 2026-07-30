use std::time::Duration;

use xredis::{self, redis};

const LOGIN_QUEUE_KEY: &str = "xkk:login_queue";

pub async fn enqueue_login(
    client: &xredis::Client,
    gid: i64,
    now_ms: i64,
    entry_ttl: Duration,
) -> xredis::Result<i64> {
    assert!(gid > 0, "login queue gid must be positive");
    assert!(!entry_ttl.is_zero(), "login queue TTL must be positive");

    const ENQUEUE: &str = r#"
local stale_before = tonumber(ARGV[2]) - tonumber(ARGV[3])
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', stale_before)
redis.call('ZADD', KEYS[1], 'NX', ARGV[2], ARGV[1])
redis.call('PEXPIRE', KEYS[1], ARGV[3])
local rank = redis.call('ZRANK', KEYS[1], ARGV[1])
return rank + 1
"#;

    let mut connection = client.connection();
    redis::Script::new(ENQUEUE)
        .key(LOGIN_QUEUE_KEY)
        .arg(gid)
        .arg(now_ms)
        .arg(entry_ttl.as_millis() as u64)
        .invoke_async(&mut connection)
        .await
        .map_err(xredis::Error::from)
}

pub async fn leave_login_queue(client: &xredis::Client, gid: i64) -> xredis::Result<()> {
    assert!(gid > 0, "login queue gid must be positive");
    let mut connection = client.connection();
    let _: usize = redis::cmd("ZREM")
        .arg(LOGIN_QUEUE_KEY)
        .arg(gid)
        .query_async(&mut connection)
        .await
        .map_err(xredis::Error::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_key_is_cluster_wide() {
        assert_eq!(LOGIN_QUEUE_KEY, "xkk:login_queue");
    }
}
