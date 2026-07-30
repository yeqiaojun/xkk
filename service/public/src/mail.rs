use std::{collections::HashSet, time::Duration};

use xframe::{
    FrameHandle, ServiceType,
    xmongo::{self, mongodb::bson::Document},
    xrpc::{RpcContext, RpcManager},
};
use xkk_cache::load_online;
use xkk_common::unix_seconds;
use xkk_persist::{load_model, save_model};
use xkk_protocol::{MsgId, code, error_status, ok_status, pb};

const MAIL_READ: i32 = 1;
const MAIL_CLAIMED: i32 = 2;
const DEFAULT_MAIL_LIFETIME: i64 = 30 * 24 * 60 * 60;
const MAX_REQUEST_MAILS: usize = 100;

#[derive(Clone)]
pub(crate) struct MailService {
    frame: FrameHandle,
    redis: xframe::xredis::Client,
    collection: xmongo::Collection<Document>,
    max_mails: usize,
    rpc_timeout: Duration,
    lock_ttl: Duration,
}

#[derive(Clone, Copy)]
pub(crate) struct MailSettings {
    pub max_mails: usize,
    pub rpc_timeout: Duration,
    pub lock_ttl: Duration,
}

impl MailService {
    pub fn new(
        frame: FrameHandle,
        redis: xframe::xredis::Client,
        collection: xmongo::Collection<Document>,
        settings: MailSettings,
    ) -> Self {
        assert!(settings.max_mails > 0, "Public max mails must be positive");
        assert!(
            !settings.rpc_timeout.is_zero(),
            "Public RPC timeout must be positive"
        );
        assert!(
            !settings.lock_ttl.is_zero(),
            "Public mail lock TTL must be positive"
        );
        Self {
            frame,
            redis,
            collection,
            max_mails: settings.max_mails,
            rpc_timeout: settings.rpc_timeout,
            lock_ttl: settings.lock_ttl,
        }
    }

    pub fn register_handlers(&self, rpc: &RpcManager) -> xframe::xrpc::Result<()> {
        let list = self.clone();
        rpc.register_pair::<pb::MailListReq, pb::MailListRsp, _, _>(
            MsgId::MailListReq.as_u32(),
            MsgId::MailListRsp.as_u32(),
            move |context, request| {
                let service = list.clone();
                async move { Ok(service.list(context, request).await) }
            },
        )?;

        let read = self.clone();
        rpc.register_pair::<pb::MailReadReq, pb::MailReadRsp, _, _>(
            MsgId::MailReadReq.as_u32(),
            MsgId::MailReadRsp.as_u32(),
            move |context, request| {
                let service = read.clone();
                async move { Ok(service.read(context, request).await) }
            },
        )?;

        let delete = self.clone();
        rpc.register_pair::<pb::MailDeleteReq, pb::MailDeleteRsp, _, _>(
            MsgId::MailDeleteReq.as_u32(),
            MsgId::MailDeleteRsp.as_u32(),
            move |context, request| {
                let service = delete.clone();
                async move { Ok(service.delete(context, request).await) }
            },
        )?;

        let claim = self.clone();
        rpc.register_pair::<pb::MailClaimReq, pb::MailClaimRsp, _, _>(
            MsgId::MailClaimReq.as_u32(),
            MsgId::MailClaimRsp.as_u32(),
            move |context, request| {
                let service = claim.clone();
                async move { Ok(service.claim(context, request).await) }
            },
        )?;

        let send = self.clone();
        rpc.register_pair::<pb::SendMailReq, pb::SendMailRsp, _, _>(
            MsgId::SendMailReq.as_u32(),
            MsgId::SendMailRsp.as_u32(),
            move |_context, request| {
                let service = send.clone();
                async move { Ok(service.send_mail(request).await) }
            },
        )?;
        Ok(())
    }

    async fn list(&self, context: RpcContext, _request: pb::MailListReq) -> pb::MailListRsp {
        let Some(gid) = player_gid(&context) else {
            return mail_list_error(code::INVALID_ARGUMENT, "missing player route");
        };
        let lock = match self.lock(gid).await {
            Ok(lock) => lock,
            Err(status) => return mail_list_status(status),
        };
        let result = self.load(gid).await;
        self.release(lock, gid).await;
        let data = match result {
            Ok(data) => data,
            Err(status) => return mail_list_status(status),
        };
        let now = unix_seconds();
        let mut mails = data
            .mails
            .into_iter()
            .filter(|mail| mail.end_time == 0 || mail.end_time > now)
            .collect::<Vec<_>>();
        mails.sort_unstable_by_key(|mail| mail.mail_id);
        pb::MailListRsp {
            status: Some(ok_status()),
            mails,
        }
    }

    async fn read(&self, context: RpcContext, request: pb::MailReadReq) -> pb::MailReadRsp {
        let Some(gid) = player_gid(&context) else {
            return mail_read_error(code::INVALID_ARGUMENT, "missing player route");
        };
        let Some(mail_ids) = valid_mail_ids(request.mail_ids) else {
            return mail_read_error(code::INVALID_ARGUMENT, "invalid mail ids");
        };
        let lock = match self.lock(gid).await {
            Ok(lock) => lock,
            Err(status) => return mail_read_status(status),
        };
        let result = async {
            let mut data = self.load(gid).await?;
            let requested = mail_ids.iter().copied().collect::<HashSet<_>>();
            let mut changed = Vec::new();
            for mail in &mut data.mails {
                if requested.contains(&mail.mail_id) && mail.state < MAIL_READ {
                    mail.state = MAIL_READ;
                    changed.push(mail.mail_id);
                }
            }
            if !changed.is_empty() {
                self.save(&data).await?;
            }
            Ok::<_, pb::Status>(changed)
        }
        .await;
        self.release(lock, gid).await;
        match result {
            Ok(mail_ids) => pb::MailReadRsp {
                status: Some(ok_status()),
                mail_ids,
            },
            Err(status) => mail_read_status(status),
        }
    }

    async fn delete(&self, context: RpcContext, request: pb::MailDeleteReq) -> pb::MailDeleteRsp {
        let Some(gid) = player_gid(&context) else {
            return mail_delete_error(code::INVALID_ARGUMENT, "missing player route");
        };
        let Some(mail_ids) = valid_mail_ids(request.mail_ids) else {
            return mail_delete_error(code::INVALID_ARGUMENT, "invalid mail ids");
        };
        let lock = match self.lock(gid).await {
            Ok(lock) => lock,
            Err(status) => return mail_delete_status(status),
        };
        let result = async {
            let mut data = self.load(gid).await?;
            let requested = mail_ids.iter().copied().collect::<HashSet<_>>();
            let mut removed = Vec::new();
            data.mails.retain(|mail| {
                if requested.contains(&mail.mail_id) {
                    removed.push(mail.mail_id);
                    false
                } else {
                    true
                }
            });
            if !removed.is_empty() {
                self.save(&data).await?;
            }
            Ok::<_, pb::Status>(removed)
        }
        .await;
        self.release(lock, gid).await;
        match result {
            Ok(mail_ids) => pb::MailDeleteRsp {
                status: Some(ok_status()),
                mail_ids,
            },
            Err(status) => mail_delete_status(status),
        }
    }

    async fn claim(&self, context: RpcContext, request: pb::MailClaimReq) -> pb::MailClaimRsp {
        let Some(gid) = player_gid(&context) else {
            return mail_claim_error(code::INVALID_ARGUMENT, "missing player route");
        };
        let Some(mail_ids) = valid_mail_ids(request.mail_ids) else {
            return mail_claim_error(code::INVALID_ARGUMENT, "invalid mail ids");
        };
        let lock = match self.lock(gid).await {
            Ok(lock) => lock,
            Err(status) => return mail_claim_status(status),
        };
        let result = self.claim_and_save(gid, &mail_ids).await;
        self.release(lock, gid).await;
        let attachments = match result {
            Ok(attachments) => attachments,
            Err(status) => return mail_claim_status(status),
        };
        if attachments.is_empty() {
            return pb::MailClaimRsp {
                status: Some(ok_status()),
                mail_ids,
                items: Vec::new(),
            };
        }

        let online = match load_online(&self.redis, gid).await {
            Ok(Some(online)) if online.logic_id > 0 => online,
            Ok(_) => {
                return mail_claim_error(
                    code::TEMPORARILY_UNAVAILABLE,
                    "Logic ownership unavailable",
                );
            }
            Err(error) => {
                xlog::error!(gid, %error, "Public claim online state load failed");
                return mail_claim_error(code::INTERNAL, "online state load failed");
            }
        };
        let response: pb::AddItemsRsp = match self
            .frame
            .call_player_to(
                ServiceType::Logic,
                online.logic_id,
                gid,
                i64::try_from(context.head.player_session)
                    .expect("validated player session fits i64"),
                MsgId::AddItemsReq.as_u32(),
                MsgId::AddItemsRsp.as_u32(),
                &pb::AddItemsReq {
                    gid,
                    items: attachments,
                    reason: format!("mail_claim:{mail_ids:?}"),
                },
                self.rpc_timeout,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                xlog::error!(gid, ?mail_ids, %error, "Public claimed mail item RPC failed");
                return mail_claim_error(code::TEMPORARILY_UNAVAILABLE, "item grant failed");
            }
        };
        let status = response
            .status
            .unwrap_or_else(|| error_status(code::INTERNAL, "Logic item response has no status"));
        if status.code != code::OK {
            xlog::error!(
                gid,
                ?mail_ids,
                status = status.code,
                "Public claimed mail item grant rejected"
            );
            return mail_claim_status(status);
        }
        pb::MailClaimRsp {
            status: Some(ok_status()),
            mail_ids,
            items: response.items,
        }
    }

    async fn claim_and_save(
        &self,
        gid: i64,
        mail_ids: &[i64],
    ) -> Result<Vec<pb::Item>, pb::Status> {
        let mut data = self.load(gid).await?;
        let now = unix_seconds();
        let mut attachments = Vec::new();
        for mail_id in mail_ids {
            let Some(mail) = data.mails.iter().find(|mail| mail.mail_id == *mail_id) else {
                return Err(error_status(code::NOT_FOUND, "mail not found"));
            };
            if (mail.end_time != 0 && mail.end_time <= now) || mail.state >= MAIL_CLAIMED {
                return Err(error_status(code::CONFLICT, "mail cannot be claimed"));
            }
            attachments.extend(mail.attachments.clone());
        }
        for mail in &mut data.mails {
            if mail_ids.contains(&mail.mail_id) {
                mail.state = MAIL_CLAIMED;
            }
        }
        self.save(&data).await?;
        Ok(attachments)
    }

    async fn send_mail(&self, request: pb::SendMailReq) -> pb::SendMailRsp {
        let Some(mut mail) = request.mail else {
            return send_mail_error(code::INVALID_ARGUMENT, "mail is required");
        };
        if request.gid <= 0
            || mail.title.is_empty()
            || mail.title.len() > 128
            || mail.content.len() > 4096
            || mail
                .attachments
                .iter()
                .any(|item| item.item_id <= 0 || item.count <= 0)
        {
            return send_mail_error(code::INVALID_ARGUMENT, "invalid mail request");
        }
        let gid = request.gid;
        let lock = match self.lock(gid).await {
            Ok(lock) => lock,
            Err(status) => return send_mail_status(status),
        };
        let result = async {
            let mut data = self.load(gid).await?;
            let mail_id = data.next_mail_id.max(1);
            data.next_mail_id = mail_id
                .checked_add(1)
                .ok_or_else(|| error_status(code::CONFLICT, "mail id exhausted"))?;
            let now = unix_seconds();
            mail.mail_id = mail_id;
            mail.send_time = if mail.send_time == 0 {
                now
            } else {
                mail.send_time
            };
            mail.end_time = if mail.end_time == 0 {
                mail.send_time + DEFAULT_MAIL_LIFETIME
            } else {
                mail.end_time
            };
            if mail.end_time <= now {
                return Err(error_status(
                    code::INVALID_ARGUMENT,
                    "mail is already expired",
                ));
            }
            mail.state = 0;
            data.mails.push(mail.clone());
            data.mails.sort_unstable_by_key(|mail| mail.mail_id);
            if data.mails.len() > self.max_mails {
                let remove = data.mails.len() - self.max_mails;
                data.mails.drain(..remove);
            }
            self.save(&data).await?;
            Ok::<_, pb::Status>(mail.clone())
        }
        .await;
        self.release(lock, gid).await;
        let mail = match result {
            Ok(mail) => mail,
            Err(status) => return send_mail_status(status),
        };
        self.notify_mail(gid, mail.clone()).await;
        pb::SendMailRsp {
            status: Some(ok_status()),
            mail_id: mail.mail_id,
        }
    }

    async fn notify_mail(&self, gid: i64, mail: pb::Mail) {
        let Ok(Some(online)) = load_online(&self.redis, gid).await else {
            return;
        };
        if online.gate_id <= 0 || online.session <= 0 {
            return;
        }
        if let Err(error) = self
            .frame
            .send_to(
                ServiceType::Gate,
                online.gate_id,
                MsgId::MailPushNtf.as_u32(),
                &pb::MailPushNtf {
                    gid,
                    mail: Some(mail),
                },
            )
            .await
        {
            xlog::debug!(gid, gate_id = online.gate_id, %error, "Public mail push failed");
        }
    }

    async fn lock(&self, gid: i64) -> Result<xframe::xredis::RedisLock, pb::Status> {
        match self
            .redis
            .try_lock(format!("xkk:mail:lock:{gid}"), self.lock_ttl)
            .await
        {
            Ok(Some(lock)) => Ok(lock),
            Ok(None) => Err(error_status(code::OVERLOADED, "mail request in progress")),
            Err(error) => {
                xlog::error!(gid, %error, "Public mail lock failed");
                Err(error_status(code::INTERNAL, "mail lock failed"))
            }
        }
    }

    async fn release(&self, lock: xframe::xredis::RedisLock, gid: i64) {
        if let Err(error) = lock.release().await {
            xlog::warn!(gid, %error, "Public mail lock release failed");
        }
    }

    async fn load(&self, gid: i64) -> Result<pb::GamerMailData, pb::Status> {
        match load_model(&self.collection, gid).await {
            Ok(Some(data)) => Ok(data),
            Ok(None) => Ok(pb::GamerMailData {
                gid,
                next_mail_id: 1,
                mails: Vec::new(),
            }),
            Err(error) => {
                xlog::error!(gid, %error, "Public mail load failed");
                Err(error_status(code::INTERNAL, "mail load failed"))
            }
        }
    }

    async fn save(&self, data: &pb::GamerMailData) -> Result<(), pb::Status> {
        save_model(&self.collection, data).await.map_err(|error| {
            xlog::error!(gid = data.gid, %error, "Public mail save failed");
            error_status(code::INTERNAL, "mail save failed")
        })
    }
}

fn player_gid(context: &RpcContext) -> Option<i64> {
    let gid = i64::try_from(context.head.gid).ok()?;
    let session = i64::try_from(context.head.player_session).ok()?;
    (gid > 0 && session > 0).then_some(gid)
}

fn valid_mail_ids(mail_ids: Vec<i64>) -> Option<Vec<i64>> {
    if mail_ids.is_empty() || mail_ids.len() > MAX_REQUEST_MAILS {
        return None;
    }
    let mut seen = HashSet::with_capacity(mail_ids.len());
    mail_ids
        .iter()
        .all(|mail_id| *mail_id > 0 && seen.insert(*mail_id))
        .then_some(mail_ids)
}

macro_rules! status_response {
    ($status_fn:ident, $error_fn:ident, $type:ty) => {
        fn $status_fn(status: pb::Status) -> $type {
            let mut response: $type = Default::default();
            response.status = Some(status);
            response
        }

        fn $error_fn(error_code: i32, message: &'static str) -> $type {
            $status_fn(error_status(error_code, message))
        }
    };
}

status_response!(mail_list_status, mail_list_error, pb::MailListRsp);
status_response!(mail_read_status, mail_read_error, pb::MailReadRsp);
status_response!(mail_delete_status, mail_delete_error, pb::MailDeleteRsp);
status_response!(mail_claim_status, mail_claim_error, pb::MailClaimRsp);
status_response!(send_mail_status, send_mail_error, pb::SendMailRsp);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_ids_must_be_positive_unique_and_bounded() {
        assert!(valid_mail_ids(vec![1, 2]).is_some());
        assert!(valid_mail_ids(vec![]).is_none());
        assert!(valid_mail_ids(vec![1, 1]).is_none());
        assert!(valid_mail_ids(vec![0]).is_none());
        assert!(valid_mail_ids(vec![1; MAX_REQUEST_MAILS + 1]).is_none());
    }
}
