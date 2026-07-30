use std::time::Duration;

use thiserror::Error;
use xframe::{
    FrameHandle, ServiceType,
    xredis::{self, redis},
};

const SERVICE_ONLINE_PREFIX: &str = "xkk:service:online";
const TTL_REFRESH_COUNT: u32 = 3;

#[derive(Debug, Error)]
pub enum ServiceOnlineError {
    #[error(transparent)]
    Frame(#[from] xframe::Error),
    #[error(transparent)]
    Redis(#[from] xredis::Error),
}

pub fn service_online_ttl(refresh_interval: Duration) -> Duration {
    assert!(
        !refresh_interval.is_zero(),
        "service online refresh interval must be positive"
    );
    refresh_interval
        .checked_mul(TTL_REFRESH_COUNT)
        .expect("service online TTL overflow")
}

fn service_online_key(cluster: &str, service_type: ServiceType) -> String {
    assert!(
        !cluster.is_empty(),
        "service online cluster must not be empty"
    );
    format!(
        "{SERVICE_ONLINE_PREFIX}:{cluster}:{}",
        service_type.as_i32()
    )
}

pub async fn publish_service_online(
    client: &xredis::Client,
    cluster: &str,
    service_type: ServiceType,
    instance_id: i32,
    online_count: i32,
    ttl: Duration,
) -> xredis::Result<()> {
    assert!(
        instance_id > 0,
        "service online instance id must be positive"
    );
    assert!(
        online_count >= 0,
        "service online count must not be negative"
    );
    let ttl_seconds = ttl.as_secs();
    assert!(
        ttl_seconds > 0,
        "service online TTL must be at least one second"
    );

    let key = service_online_key(cluster, service_type);
    let mut connection = client.connection();
    let mut pipeline = redis::pipe();
    pipeline
        .atomic()
        .cmd("HSET")
        .arg(&key)
        .arg(instance_id)
        .arg(online_count)
        .ignore()
        .cmd("EXPIRE")
        .arg(&key)
        .arg(ttl_seconds)
        .ignore();
    let _: () = pipeline
        .query_async(&mut connection)
        .await
        .map_err(xredis::Error::from)?;
    Ok(())
}

async fn load_service_online_counts(
    client: &xredis::Client,
    cluster: &str,
    service_type: ServiceType,
    instances: &[xframe::xservice::ServiceInstance],
) -> xredis::Result<Vec<Option<i32>>> {
    if instances.is_empty() {
        return Ok(Vec::new());
    }

    let mut command = redis::cmd("HMGET");
    command.arg(service_online_key(cluster, service_type));
    for instance in instances {
        command.arg(instance.instance_id);
    }
    let mut connection = client.connection();
    command
        .query_async(&mut connection)
        .await
        .map_err(xredis::Error::from)
}

pub async fn refresh_service_online(
    frame: &FrameHandle,
    client: &xredis::Client,
    cluster: &str,
    service_type: ServiceType,
) -> Result<(), ServiceOnlineError> {
    let mut instances = frame.service_instances(service_type)?;
    let online_counts =
        load_service_online_counts(client, cluster, service_type, &instances).await?;
    for (instance, online_count) in instances.iter_mut().zip(online_counts) {
        if let Some(online_count) = online_count {
            instance.online_count = online_count;
        }
    }
    frame.update_service_loads(instances)?;
    Ok(())
}

pub async fn delete_service_online(
    client: &xredis::Client,
    cluster: &str,
    service_type: ServiceType,
    instance_id: i32,
) -> xredis::Result<()> {
    assert!(
        instance_id > 0,
        "service online instance id must be positive"
    );
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
        assert_eq!(
            service_online_key("local", ServiceType::Logic),
            "xkk:service:online:local:2"
        );
    }

    #[test]
    fn service_online_ttl_covers_three_refreshes() {
        assert_eq!(
            service_online_ttl(Duration::from_secs(3)),
            Duration::from_secs(9)
        );
    }
}
