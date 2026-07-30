use std::{
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, StatusCode, Uri, header::CONTENT_TYPE};
use hyper_util::{
    client::legacy::{Client as HttpClient, connect::HttpConnector},
    rt::TokioExecutor,
};
use serde::{Serialize, de::DeserializeOwned};
use tokio::time::{Instant, sleep, timeout};
use xkk_config::{RobotConfig, RobotTransport};
use xkk_protocol::{code, pb};

use crate::{
    error::{Result, failure},
    gate::{connect_gate, login_gate, resolve_endpoint, select_endpoint},
};

const LOGIN_PATH: &str = "/v1/auth/login";
const USE_ROLE_PATH: &str = "/v1/auth/use-role";

type JsonClient = HttpClient<HttpConnector, Full<Bytes>>;

#[derive(Debug, PartialEq, Eq)]
pub struct LoginResult {
    pub gid: i64,
    pub session_id: i64,
    pub logic_id: i32,
    pub public_id: i32,
    pub transport: RobotTransport,
    pub host: String,
    pub port: u16,
}

pub async fn login(config: &RobotConfig) -> Result<LoginResult> {
    let deadline = Instant::now() + Duration::from_secs(config.timeout_seconds);
    let http = HttpClient::builder(TokioExecutor::new()).build(HttpConnector::new());
    let auth = post_json::<_, pb::AuthLoginRsp>(
        &http,
        &config.auth_url,
        LOGIN_PATH,
        &pb::AuthLoginReq {
            account: config.account.clone(),
            credential: config.credential.clone(),
            device: Some(pb::DeviceInfo {
                device_id: config.device_id.clone(),
                platform: config.platform.clone(),
                client_version: config.client_version.clone(),
            }),
        },
        deadline,
        "Auth login",
    )
    .await?;
    let (gid, token) = validate_auth_login(&config.account, auth)?;

    let endpoints = admit_role(&http, config, gid, &token, deadline).await?;
    let endpoint = select_endpoint(&endpoints, config.transport)?;
    let addr = resolve_endpoint(&endpoint, deadline).await?;
    let gate = connect_gate(&endpoint, addr, deadline)?;
    login_gate(gate, config, gid, token, endpoint, deadline).await
}

async fn admit_role(
    http: &JsonClient,
    config: &RobotConfig,
    gid: i64,
    token: &str,
    deadline: Instant,
) -> Result<Vec<pb::Endpoint>> {
    let request = pb::AuthUseRoleReq {
        gid,
        token: token.to_string(),
        device_id: config.device_id.clone(),
    };

    loop {
        let response = post_json::<_, pb::AuthUseRoleRsp>(
            http,
            &config.auth_url,
            USE_ROLE_PATH,
            &request,
            deadline,
            "Auth use-role",
        )
        .await?;
        require_ok(response.status.as_ref(), "Auth use-role")?;
        if response.gid != gid {
            return Err(failure(format!(
                "Auth use-role returned gid {}, expected {gid}",
                response.gid
            )));
        }

        let Some(queue) = response.queue else {
            if response.endpoints.is_empty() {
                return Err(failure("Auth use-role returned no Gate endpoints"));
            }
            return Ok(response.endpoints);
        };
        if !response.endpoints.is_empty() {
            return Err(failure(
                "Auth use-role returned both queue state and Gate endpoints",
            ));
        }
        wait_for_queue(&queue, deadline).await?;
    }
}

async fn wait_for_queue(queue: &pb::LoginQueue, deadline: Instant) -> Result<()> {
    if queue.position <= 0 || queue.next_request_time <= 0 {
        return Err(failure("Auth use-role returned invalid queue state"));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| failure(format!("read system clock: {error}")))?
        .as_secs();
    let next = u64::try_from(queue.next_request_time)
        .map_err(|_| failure("Auth use-role returned invalid next_request_time"))?;
    let wait = Duration::from_secs(next.saturating_sub(now));
    let remaining = remaining(deadline, "Auth use-role queue")?;
    if wait >= remaining {
        return Err(failure("Auth use-role queue timed out"));
    }
    sleep(wait).await;
    Ok(())
}

async fn post_json<T, R>(
    client: &JsonClient,
    base_url: &str,
    path: &str,
    value: &T,
    deadline: Instant,
    operation: &str,
) -> Result<R>
where
    T: Serialize,
    R: DeserializeOwned,
{
    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    let uri: Uri = url
        .parse()
        .map_err(|error| failure(format!("invalid {operation} URL {url}: {error}")))?;
    let body = serde_json::to_vec(value)
        .map_err(|error| failure(format!("encode {operation} request: {error}")))?;
    let request = Request::post(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body)))
        .map_err(|error| failure(format!("build {operation} request: {error}")))?;

    let response = before_deadline(deadline, operation, client.request(request))
        .await?
        .map_err(|error| failure(format!("send {operation} request: {error}")))?;
    let status = response.status();
    let body = before_deadline(deadline, operation, response.into_body().collect())
        .await?
        .map_err(|error| failure(format!("read {operation} response: {error}")))?
        .to_bytes();
    if status != StatusCode::OK {
        return Err(failure(format!(
            "{operation} returned HTTP {status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    serde_json::from_slice(&body)
        .map_err(|error| failure(format!("decode {operation} response: {error}")))
}

fn validate_auth_login(account: &str, response: pb::AuthLoginRsp) -> Result<(i64, String)> {
    require_ok(response.status.as_ref(), "Auth login")?;
    if response.account != account {
        return Err(failure(format!(
            "Auth login returned account {}, expected {account}",
            response.account
        )));
    }
    let role = response
        .roles
        .first()
        .ok_or_else(|| failure("Auth login returned no role"))?;
    if role.gid <= 0 {
        return Err(failure("Auth login returned invalid gid"));
    }
    if response.token.is_empty() {
        return Err(failure("Auth login returned an empty token"));
    }
    Ok((role.gid, response.token))
}

pub(crate) fn require_ok(status: Option<&pb::Status>, operation: &str) -> Result<()> {
    match status {
        Some(status) if status.code == code::OK => Ok(()),
        Some(status) => Err(failure(format!(
            "{operation} failed: code={} message={}",
            status.code, status.message
        ))),
        None => Err(failure(format!("{operation} returned no status"))),
    }
}

pub(crate) async fn before_deadline<T>(
    deadline: Instant,
    operation: &str,
    future: impl Future<Output = T>,
) -> Result<T> {
    timeout(remaining(deadline, operation)?, future)
        .await
        .map_err(|_| failure(format!("{operation} timed out")))
}

pub(crate) fn remaining(deadline: Instant, operation: &str) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| failure(format!("{operation} timed out")))
}
