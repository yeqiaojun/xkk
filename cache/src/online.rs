use std::collections::HashMap;

use thiserror::Error;
use xredis::{self, redis};

const ONLINE_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OnlineData {
    pub account: String,
    pub gid: i64,
    pub token: String,
    pub session: i64,
    pub login_time: i64,
    pub logout_time: i64,
    pub public_id: i32,
    pub gate_id: i32,
    pub logic_id: i32,
}

#[derive(Debug, Error)]
pub enum OnlineError {
    #[error(transparent)]
    Redis(#[from] xredis::Error),
    #[error("invalid Redis field {field}={value}")]
    InvalidField { field: &'static str, value: String },
    #[error("online gid must be positive")]
    InvalidGid,
}

pub type Result<T> = std::result::Result<T, OnlineError>;

pub fn online_key(gid: i64) -> String {
    format!("gamer:{gid}")
}

pub async fn load_online(client: &xredis::Client, gid: i64) -> Result<Option<OnlineData>> {
    if gid <= 0 {
        return Err(OnlineError::InvalidGid);
    }
    let mut connection = client.connection();
    let fields: HashMap<String, String> = redis::cmd("HGETALL")
        .arg(online_key(gid))
        .query_async(&mut connection)
        .await
        .map_err(xredis::Error::from)?;
    if fields.is_empty() {
        return Ok(None);
    }

    Ok(Some(OnlineData {
        account: fields.get("acc").cloned().unwrap_or_default(),
        gid: parse_i64(&fields, "gid", gid)?,
        token: fields.get("token").cloned().unwrap_or_default(),
        session: parse_i64(&fields, "sess", 0)?,
        login_time: parse_i64(&fields, "lgin", 0)?,
        logout_time: parse_i64(&fields, "lgou", 0)?,
        public_id: parse_i32(&fields, "psid", 0)?,
        gate_id: parse_i32(&fields, "gsid", 0)?,
        logic_id: parse_i32(&fields, "lsid", 0)?,
    }))
}

pub async fn save_online(client: &xredis::Client, data: &OnlineData) -> Result<()> {
    if data.gid <= 0 {
        return Err(OnlineError::InvalidGid);
    }
    let mut connection = client.connection();
    let mut pipeline = redis::pipe();
    pipeline
        .atomic()
        .cmd("HSET")
        .arg(online_key(data.gid))
        .arg("acc")
        .arg(&data.account)
        .arg("gid")
        .arg(data.gid)
        .arg("token")
        .arg(&data.token)
        .arg("sess")
        .arg(data.session)
        .arg("lgin")
        .arg(data.login_time)
        .arg("lgou")
        .arg(data.logout_time)
        .arg("psid")
        .arg(data.public_id)
        .arg("gsid")
        .arg(data.gate_id)
        .arg("lsid")
        .arg(data.logic_id)
        .ignore()
        .cmd("EXPIRE")
        .arg(online_key(data.gid))
        .arg(ONLINE_TTL_SECONDS)
        .ignore();
    let _: () = pipeline
        .query_async(&mut connection)
        .await
        .map_err(xredis::Error::from)?;
    Ok(())
}

pub async fn set_token(
    client: &xredis::Client,
    gid: i64,
    account: &str,
    token: &str,
) -> Result<()> {
    if gid <= 0 {
        return Err(OnlineError::InvalidGid);
    }
    let mut connection = client.connection();
    let mut pipeline = redis::pipe();
    pipeline
        .atomic()
        .cmd("HSET")
        .arg(online_key(gid))
        .arg("acc")
        .arg(account)
        .arg("gid")
        .arg(gid)
        .arg("token")
        .arg(token)
        .ignore()
        .cmd("EXPIRE")
        .arg(online_key(gid))
        .arg(ONLINE_TTL_SECONDS)
        .ignore();
    let _: () = pipeline
        .query_async(&mut connection)
        .await
        .map_err(xredis::Error::from)?;
    Ok(())
}

pub async fn set_logic_owner(client: &xredis::Client, gid: i64, logic_id: i32) -> Result<()> {
    if gid <= 0 {
        return Err(OnlineError::InvalidGid);
    }
    let mut connection = client.connection();
    let _: () = redis::cmd("HSET")
        .arg(online_key(gid))
        .arg("lsid")
        .arg(logic_id)
        .query_async(&mut connection)
        .await
        .map_err(xredis::Error::from)?;
    Ok(())
}

pub async fn clear_gate_by_session(
    client: &xredis::Client,
    gid: i64,
    session: i64,
    logout_time: i64,
) -> Result<bool> {
    const CLEAR: &str = r#"
local current = redis.call('HGET', KEYS[1], 'sess')
if not current or tonumber(current) ~= tonumber(ARGV[1]) then
    return 0
end
redis.call('HSET', KEYS[1], 'gsid', 0, 'sess', 0, 'lgou', ARGV[2])
return 1
"#;

    let mut connection = client.connection();
    let cleared: i64 = redis::Script::new(CLEAR)
        .key(online_key(gid))
        .arg(session)
        .arg(logout_time)
        .invoke_async(&mut connection)
        .await
        .map_err(xredis::Error::from)?;
    Ok(cleared == 1)
}

fn parse_i64(fields: &HashMap<String, String>, field: &'static str, default: i64) -> Result<i64> {
    let Some(value) = fields.get(field) else {
        return Ok(default);
    };
    value.parse().map_err(|_| OnlineError::InvalidField {
        field,
        value: value.clone(),
    })
}

fn parse_i32(fields: &HashMap<String, String>, field: &'static str, default: i32) -> Result<i32> {
    let Some(value) = fields.get(field) else {
        return Ok(default);
    };
    value.parse().map_err(|_| OnlineError::InvalidField {
        field,
        value: value.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_fields_keep_product_names_without_module_flag() {
        let fields = HashMap::from([
            ("acc".to_string(), "account".to_string()),
            ("gid".to_string(), "7".to_string()),
            ("sess".to_string(), "8".to_string()),
            ("psid".to_string(), "9".to_string()),
            ("gsid".to_string(), "10".to_string()),
            ("lsid".to_string(), "11".to_string()),
        ]);

        assert_eq!(parse_i64(&fields, "gid", 0).unwrap(), 7);
        assert_eq!(parse_i32(&fields, "lsid", 0).unwrap(), 11);
        assert!(!fields.contains_key("modf"));
    }
}
