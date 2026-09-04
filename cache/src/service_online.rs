use std::time::Duration;

use xredis::{self, redis};

const SERVICE_ONLINE_PREFIX: &str = "xkk:service:online";
const TTL_REFRESH_COUNT: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceOnlineCount {
    pub instance_id: i32,
    pub online_count: i32,
}

pub fn service_online_ttl(refresh_interval: Duration) -> Duration {
    assert!(!refresh_interval.is_zero(), "service online refresh interval must be positive");
    refresh_interval.checked_mul(TTL_REFRESH_COUNT).expect("service online TTL overflow")
}

fn service_online_key(cluster: &str, service_type: i32) -> String {
    assert!(!cluster.is_empty(), "service online cluster must not be empty");
    assert!(service_type > 0, "service type must be positive");
    format!("{SERVICE_ONLINE_PREFIX}:{cluster}:{service_type}")
}

pub async fn publish_service_online(
    client: &xredis::Client,
    cluster: &str,
    service_type: i32,
    instance_id: i32,
    online_count: i32,
    ttl: Duration,
) -> xredis::Result<()> {
    assert!(instance_id > 0, "service online instance id must be positive");
    assert!(online_count >= 0, "service online count must not be negative");
    let ttl_seconds = ttl.as_secs();
    assert!(ttl_seconds > 0, "service online TTL must be at least one second");

    let key = service_online_key(cluster, service_type);
    let mut connection = client.connection();
    let mut pipeline = redis::pipe();
    pipeline.atomic().cmd("HSET").arg(&key).arg(instance_id).arg(online_count).ignore().cmd("EXPIRE").arg(&key).arg(ttl_seconds).ignore();
    let _: () = pipeline.query_async(&mut connection).await.map_err(xredis::Error::from)?;
    Ok(())
}

pub async fn load_service_online_counts(
    client: &xredis::Client,
    cluster: &str,
    service_type: i32,
    instance_ids: impl IntoIterator<Item = i32>,
) -> xredis::Result<Vec<ServiceOnlineCount>> {
    let instance_ids = instance_ids.into_iter().collect::<Vec<_>>();
    if instance_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut command = redis::cmd("HMGET");
    command.arg(service_online_key(cluster, service_type));
    for instance_id in &instance_ids {
        command.arg(instance_id);
    }
    let mut connection = client.connection();
    let values: Vec<Option<i32>> = command.query_async(&mut connection).await.map_err(xredis::Error::from)?;
    Ok(instance_ids
        .into_iter()
        .zip(values)
        .filter_map(|(instance_id, online_count)| online_count.map(|online_count| ServiceOnlineCount { instance_id, online_count }))
        .collect())
}

pub async fn delete_service_online(client: &xredis::Client, cluster: &str, service_type: i32, instance_id: i32) -> xredis::Result<()> {
    assert!(instance_id > 0, "service online instance id must be positive");
    let mut connection = client.connection();
    let _: usize = redis::cmd("HDEL")
        .arg(service_online_key(cluster, service_type))
        .arg(instance_id)
        .query_async(&mut connection)
        .await
        .map_err(xredis::Error::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_online_hash_is_cluster_and_type_scoped() {
        assert_eq!(service_online_key("local", 2), "xkk:service:online:local:2");
    }

    #[test]
    fn service_online_ttl_covers_three_refreshes() {
        assert_eq!(service_online_ttl(Duration::from_secs(3)), Duration::from_secs(9));
    }
}
