use std::{collections::HashSet, time::Duration};

use xframe::{
    FrameHandle,
    xrpc::{RpcContext, RpcManager},
};
use xkk_cache::load_online;
use xkk_common::unix_seconds;
use xkk_persist::{PublicPlayerCacheError, PublicPlayers};
use xkk_protocol::{code, error_status, ok_status, pb};

const MAIL_READ: i32 = 1;
const MAIL_CLAIMED: i32 = 2;
const DEFAULT_MAIL_LIFETIME: i64 = 30 * 24 * 60 * 60;
const MAX_REQUEST_MAILS: usize = 100;
// Mail retention and the internal RPC timeout are product conventions. They
// are deliberately changed through code review.
const MAX_MAILS_PER_PLAYER: usize = 200;
const RPC_CALL_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub(crate) struct MailService {
    frame: FrameHandle,
    redis: xredis::Client,
    players: PublicPlayers,
}

impl MailService {
    pub fn new(frame: FrameHandle, redis: xredis::Client, players: PublicPlayers) -> Self {
        Self { frame, redis, players }
    }

    pub fn register_handlers(&self, rpc: &RpcManager) -> xframe::xrpc::Result<()> {
        let list = self.clone();
        rpc.register::<pb::MailListReq, _, _>(move |context, request| {
            let service = list.clone();
            async move { Ok(service.list(context, request).await) }
        })?;

        let read = self.clone();
        rpc.register::<pb::MailReadReq, _, _>(move |context, request| {
            let service = read.clone();
            async move { Ok(service.read(context, request).await) }
        })?;

        let delete = self.clone();
        rpc.register::<pb::MailDeleteReq, _, _>(move |context, request| {
            let service = delete.clone();
            async move { Ok(service.delete(context, request).await) }
        })?;

        let claim = self.clone();
        rpc.register::<pb::MailClaimReq, _, _>(move |context, request| {
            let service = claim.clone();
            async move { Ok(service.claim(context, request).await) }
        })?;

        let send = self.clone();
        rpc.register::<pb::SendMailReq, _, _>(move |_context, request| {
            let service = send.clone();
            async move { Ok(service.send_mail(request).await) }
        })?;
        Ok(())
    }

    async fn list(&self, context: RpcContext, _request: pb::MailListReq) -> pb::MailListRsp {
        let Some(gid) = player_gid(&context) else {
            return mail_list_error(code::INVALID_ARGUMENT, "missing player route");
        };
        let result = self
            .players
            .read(gid, |data| {
                let now = unix_seconds();
                let mut mails =
                    mail(data).mails.iter().filter(|mail| mail.end_time == 0 || mail.end_time > now).cloned().collect::<Vec<_>>();
                mails.sort_unstable_by_key(|mail| mail.mail_id);
                mails
            })
            .await;
        match result {
            Ok(mails) => pb::MailListRsp { status: Some(ok_status()), mails },
            Err(error) => mail_list_status(cache_status(gid, "list", error)),
        }
    }

    async fn read(&self, context: RpcContext, request: pb::MailReadReq) -> pb::MailReadRsp {
        let Some(gid) = player_gid(&context) else {
            return mail_read_error(code::INVALID_ARGUMENT, "missing player route");
        };
        let Some(mail_ids) = valid_mail_ids(request.mail_ids) else {
            return mail_read_error(code::INVALID_ARGUMENT, "invalid mail ids");
        };
        let result = self
            .players
            .update(gid, |data| {
                let requested = mail_ids.iter().copied().collect::<HashSet<_>>();
                let mut changed = Vec::new();
                for mail in &mut mail_mut(data).mails {
                    if requested.contains(&mail.mail_id) && mail.state < MAIL_READ {
                        mail.state = MAIL_READ;
                        changed.push(mail.mail_id);
                    }
                }
                let dirty = !changed.is_empty();
                (changed, dirty)
            })
            .await;
        match result {
            Ok(mail_ids) => pb::MailReadRsp { status: Some(ok_status()), mail_ids },
            Err(error) => mail_read_status(cache_status(gid, "read", error)),
        }
    }

    async fn delete(&self, context: RpcContext, request: pb::MailDeleteReq) -> pb::MailDeleteRsp {
        let Some(gid) = player_gid(&context) else {
            return mail_delete_error(code::INVALID_ARGUMENT, "missing player route");
        };
        let Some(mail_ids) = valid_mail_ids(request.mail_ids) else {
            return mail_delete_error(code::INVALID_ARGUMENT, "invalid mail ids");
        };
        let result = self
            .players
            .update(gid, |data| {
                let requested = mail_ids.iter().copied().collect::<HashSet<_>>();
                let mut removed = Vec::new();
                mail_mut(data).mails.retain(|mail| {
                    if requested.contains(&mail.mail_id) {
                        removed.push(mail.mail_id);
                        false
                    } else {
                        true
                    }
                });
                let dirty = !removed.is_empty();
                (removed, dirty)
            })
            .await;
        match result {
            Ok(mail_ids) => pb::MailDeleteRsp { status: Some(ok_status()), mail_ids },
            Err(error) => mail_delete_status(cache_status(gid, "delete", error)),
        }
    }

    async fn claim(&self, context: RpcContext, request: pb::MailClaimReq) -> pb::MailClaimRsp {
        let Some(gid) = player_gid(&context) else {
            return mail_claim_error(code::INVALID_ARGUMENT, "missing player route");
        };
        let Some(mail_ids) = valid_mail_ids(request.mail_ids) else {
            return mail_claim_error(code::INVALID_ARGUMENT, "invalid mail ids");
        };
        let result = self.players.update(gid, |data| claim_mails(mail_mut(data), &mail_ids)).await;
        let attachments = match result {
            Ok(Ok(attachments)) => attachments,
            Ok(Err(status)) => return mail_claim_status(status),
            Err(error) => return mail_claim_status(cache_status(gid, "claim", error)),
        };
        if attachments.is_empty() {
            return pb::MailClaimRsp { status: Some(ok_status()), mail_ids, items: Vec::new() };
        }

        let online = match load_online(&self.redis, gid).await {
            Ok(Some(online)) if online.logic_id > 0 => online,
            Ok(_) => {
                return mail_claim_error(code::TEMPORARILY_UNAVAILABLE, "Logic ownership unavailable");
            }
            Err(error) => {
                tracing::error!(gid, %error, "Public claim online state load failed");
                return mail_claim_error(code::INTERNAL, "online state load failed");
            }
        };
        let response: pb::AddItemsRsp = match self
            .frame
            .call_routed_to(
                xkk_common::service_type::LOGIC,
                online.logic_id,
                context.head.gid,
                context.head.player_session,
                &pb::AddItemsReq { gid, items: attachments, reason: format!("mail_claim:{mail_ids:?}") },
                RPC_CALL_TIMEOUT,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(gid, ?mail_ids, %error, "Public claimed mail item RPC failed");
                return mail_claim_error(code::TEMPORARILY_UNAVAILABLE, "item grant failed");
            }
        };
        let status = response.status.unwrap_or_else(|| error_status(code::INTERNAL, "Logic item response has no status"));
        if status.code != code::OK {
            tracing::error!(gid, ?mail_ids, status = status.code, "Public claimed mail item grant rejected");
            return mail_claim_status(status);
        }
        pb::MailClaimRsp { status: Some(ok_status()), mail_ids, items: response.items }
    }

    async fn send_mail(&self, request: pb::SendMailReq) -> pb::SendMailRsp {
        let Some(mut message) = request.mail else {
            return send_mail_error(code::INVALID_ARGUMENT, "mail is required");
        };
        if request.gid <= 0
            || message.title.is_empty()
            || message.title.len() > 128
            || message.content.len() > 4096
            || message.attachments.iter().any(|item| item.item_id <= 0 || item.count <= 0)
        {
            return send_mail_error(code::INVALID_ARGUMENT, "invalid mail request");
        }
        let gid = request.gid;
        let result = self
            .players
            .update(gid, |data| {
                let now = unix_seconds();
                let data = mail_mut(data);
                let mail_id = new_mail_id();
                message.mail_id = mail_id;
                message.send_time = if message.send_time == 0 { now } else { message.send_time };
                message.end_time = if message.end_time == 0 { message.send_time + DEFAULT_MAIL_LIFETIME } else { message.end_time };
                if message.end_time <= now {
                    return (Err(error_status(code::INVALID_ARGUMENT, "mail is already expired")), false);
                }

                message.state = 0;
                data.mails.push(message.clone());
                data.mails.sort_unstable_by_key(|mail| mail.mail_id);
                if data.mails.len() > MAX_MAILS_PER_PLAYER {
                    let remove = data.mails.len() - MAX_MAILS_PER_PLAYER;
                    tracing::error!(gid, retained = MAX_MAILS_PER_PLAYER, removed = remove, "Public mail retention hard limit exceeded");
                    data.mails.drain(..remove);
                }
                (Ok(message.clone()), true)
            })
            .await;
        let message = match result {
            Ok(Ok(message)) => message,
            Ok(Err(status)) => return send_mail_status(status),
            Err(error) => return send_mail_status(cache_status(gid, "send", error)),
        };
        self.notify_mail(gid, message.clone()).await;
        pb::SendMailRsp { status: Some(ok_status()), mail_id: message.mail_id }
    }

    async fn notify_mail(&self, gid: i64, mail: pb::Mail) {
        let Ok(Some(online)) = load_online(&self.redis, gid).await else {
            return;
        };
        if online.gate_id <= 0 || online.session <= 0 {
            return;
        }
        if let Err(error) =
            self.frame.send_to(xkk_common::service_type::GATE, online.gate_id, &pb::MailPushNtf { gid, mail: Some(mail) }).await
        {
            tracing::debug!(gid, gate_id = online.gate_id, %error, "Public mail push failed");
        }
    }
}

fn claim_mails(data: &mut pb::MailData, mail_ids: &[i64]) -> (Result<Vec<pb::Item>, pb::Status>, bool) {
    let now = unix_seconds();
    let mut attachments = Vec::new();
    for mail_id in mail_ids {
        let Some(mail) = data.mails.iter().find(|mail| mail.mail_id == *mail_id) else {
            return (Err(error_status(code::NOT_FOUND, "mail not found")), false);
        };
        if (mail.end_time != 0 && mail.end_time <= now) || mail.state >= MAIL_CLAIMED {
            return (Err(error_status(code::CONFLICT, "mail cannot be claimed")), false);
        }
        attachments.extend(mail.attachments.clone());
    }
    for mail in &mut data.mails {
        if mail_ids.contains(&mail.mail_id) {
            mail.state = MAIL_CLAIMED;
        }
    }
    (Ok(attachments), true)
}

fn mail(data: &pb::PublicPlayerData) -> &pb::MailData {
    data.mail.as_ref().expect("persist normalizes Public player Mail data")
}

fn mail_mut(data: &mut pb::PublicPlayerData) -> &mut pb::MailData {
    data.mail.as_mut().expect("persist normalizes Public player Mail data")
}

fn cache_status(gid: i64, operation: &'static str, error: PublicPlayerCacheError) -> pb::Status {
    tracing::error!(gid, operation, %error, "Public player cache operation failed");
    error_status(code::INTERNAL, "Public player data unavailable")
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
    mail_ids.iter().all(|mail_id| *mail_id > 0 && seen.insert(*mail_id)).then_some(mail_ids)
}

fn new_mail_id() -> i64 {
    xfastid::gen_int64_id()
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

    #[test]
    fn new_mail_ids_use_the_global_fastid_generator() {
        let first = new_mail_id();
        let second = new_mail_id();

        assert!(first > 0);
        assert!(second > first);
    }

    #[test]
    fn claim_validates_every_mail_before_mutating() {
        let mut data = pb::MailData {
            mails: vec![
                pb::Mail { mail_id: 1, attachments: vec![pb::Item { item_id: 7, count: 2, change: 0 }], ..Default::default() },
                pb::Mail { mail_id: 2, state: MAIL_CLAIMED, ..Default::default() },
            ],
        };

        let (result, changed) = claim_mails(&mut data, &[1, 2]);

        assert!(result.is_err());
        assert!(!changed);
        assert_eq!(data.mails[0].state, 0);
    }
}
