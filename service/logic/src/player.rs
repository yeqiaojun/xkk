use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    time::{Duration, Instant},
};

use prost::Message;
use thiserror::Error;
use xframe::{FrameHandle, ServiceType, xmongo, xrpc::RpcManager};
use xkk_cache::set_logic_owner;
use xkk_persist::{load_model, save_model};
use xkk_protocol::{code, error_status, ok_status, pb};

use crate::{
    Completed, LogicCall, LogicCallError, LogicRuntime, LogicState, Persistence, RejectReason,
    SavePlayer, stats::LoginMetrics,
};

const KIB: usize = 1024;

#[derive(Debug, Error)]
pub(crate) enum PlayerError {
    #[error(transparent)]
    Mongo(#[from] xmongo::Error),
}

#[derive(Clone, Copy)]
struct PlayerSession {
    gate_id: i32,
    session_id: i64,
}

pub(crate) struct PlayerState {
    data: pb::PlayerData,
    session: Option<PlayerSession>,
    dirty: bool,
}

impl LogicState for PlayerState {
    fn is_dirty(&self) -> bool {
        self.dirty
    }
}

impl PlayerState {
    fn new(data: pb::PlayerData) -> Self {
        Self {
            data,
            session: None,
            dirty: false,
        }
    }

    fn login(&mut self, gate_id: i32, session_id: i64) -> LoginResult {
        let became_online = self.session.is_none();
        self.session = Some(PlayerSession {
            gate_id,
            session_id,
        });
        LoginResult {
            became_online,
            response: pb::LogicLoginRsp {
                status: Some(ok_status()),
                logic_id: 0,
                old_gate_id: 0,
                old_player_session: 0,
                player: self.data.profile.clone(),
                items: self.items(),
            },
        }
    }

    fn disconnect(&mut self, gate_id: i32, session_id: i64) -> bool {
        let matches = self
            .session
            .is_some_and(|current| current.gate_id == gate_id && current.session_id == session_id);
        if matches {
            self.session = None;
        }
        matches
    }

    fn player_info(&self) -> pb::PlayerInfoRsp {
        pb::PlayerInfoRsp {
            status: Some(ok_status()),
            player: self.data.profile.clone(),
            items: self.items(),
        }
    }

    fn use_item(&mut self, request: pb::UseItemReq) -> pb::UseItemRsp {
        if request.item_id <= 0 || request.count <= 0 {
            return pb::UseItemRsp {
                status: Some(error_status(code::INVALID_ARGUMENT, "invalid item request")),
                item: None,
            };
        }
        let current = self.data.items.get(&request.item_id).copied().unwrap_or(0);
        if current < request.count {
            return pb::UseItemRsp {
                status: Some(error_status(code::INSUFFICIENT_ITEMS, "insufficient items")),
                item: Some(pb::Item {
                    item_id: request.item_id,
                    count: current,
                    change: 0,
                }),
            };
        }
        let remaining = current - request.count;
        if remaining == 0 {
            self.data.items.remove(&request.item_id);
        } else {
            self.data.items.insert(request.item_id, remaining);
        }
        self.dirty = true;
        pb::UseItemRsp {
            status: Some(ok_status()),
            item: Some(pb::Item {
                item_id: request.item_id,
                count: remaining,
                change: -request.count,
            }),
        }
    }

    fn add_items(&mut self, items: Vec<pb::Item>) -> pb::AddItemsRsp {
        if items.is_empty()
            || items
                .iter()
                .any(|item| item.item_id <= 0 || item.count <= 0)
        {
            return pb::AddItemsRsp {
                status: Some(error_status(
                    code::INVALID_ARGUMENT,
                    "invalid add-items request",
                )),
                items: Vec::new(),
            };
        }
        let mut changes = aggregate_items(items);
        for (item_id, amount) in &changes {
            let current = self.data.items.get(item_id).copied().unwrap_or(0);
            let Some(next) = current.checked_add(*amount) else {
                return pb::AddItemsRsp {
                    status: Some(error_status(code::CONFLICT, "item count overflow")),
                    items: Vec::new(),
                };
            };
            self.data.items.insert(*item_id, next);
        }
        self.dirty = true;
        let items = changes
            .drain()
            .map(|(item_id, change)| pb::Item {
                item_id,
                count: self.data.items[&item_id],
                change,
            })
            .collect();
        pb::AddItemsRsp {
            status: Some(ok_status()),
            items,
        }
    }

    fn remove_items(&mut self, items: Vec<pb::Item>) -> pb::RemoveItemsRsp {
        if items.is_empty()
            || items
                .iter()
                .any(|item| item.item_id <= 0 || item.count <= 0)
        {
            return pb::RemoveItemsRsp {
                status: Some(error_status(
                    code::INVALID_ARGUMENT,
                    "invalid remove-items request",
                )),
                items: Vec::new(),
            };
        }
        let changes = aggregate_items(items);
        if changes
            .iter()
            .any(|(item_id, amount)| self.data.items.get(item_id).copied().unwrap_or(0) < *amount)
        {
            return pb::RemoveItemsRsp {
                status: Some(error_status(code::INSUFFICIENT_ITEMS, "insufficient items")),
                items: Vec::new(),
            };
        }
        let mut result = Vec::with_capacity(changes.len());
        for (item_id, amount) in changes {
            let remaining = self.data.items[&item_id] - amount;
            if remaining == 0 {
                self.data.items.remove(&item_id);
            } else {
                self.data.items.insert(item_id, remaining);
            }
            result.push(pb::Item {
                item_id,
                count: remaining,
                change: -amount,
            });
        }
        self.dirty = true;
        pb::RemoveItemsRsp {
            status: Some(ok_status()),
            items: result,
        }
    }

    fn check_items(&self, items: Vec<pb::Item>) -> pb::CheckItemsRsp {
        if items.is_empty()
            || items
                .iter()
                .any(|item| item.item_id <= 0 || item.count <= 0)
        {
            return pb::CheckItemsRsp {
                status: Some(error_status(
                    code::INVALID_ARGUMENT,
                    "invalid check-items request",
                )),
                enough: false,
            };
        }
        let enough = aggregate_items(items)
            .into_iter()
            .all(|(item_id, amount)| self.data.items.get(&item_id).copied().unwrap_or(0) >= amount);
        pb::CheckItemsRsp {
            status: Some(ok_status()),
            enough,
        }
    }

    fn items(&self) -> Vec<pb::Item> {
        let mut items = self
            .data
            .items
            .iter()
            .map(|(item_id, count)| pb::Item {
                item_id: *item_id,
                count: *count,
                change: 0,
            })
            .collect::<Vec<_>>();
        items.sort_unstable_by_key(|item| item.item_id);
        items
    }
}

struct LoginResult {
    became_online: bool,
    response: pb::LogicLoginRsp,
}

pub(crate) fn persistence(
    collection: xmongo::Collection<xmongo::mongodb::bson::Document>,
    metrics: Arc<LoginMetrics>,
) -> Persistence<PlayerState, PlayerError> {
    let load_collection = collection.clone();
    let load_metrics = metrics;
    Persistence::new(
        move |gid| {
            let collection = load_collection.clone();
            let metrics = load_metrics.clone();
            async move {
                let find_started = Instant::now();
                let loaded = load_model::<pb::PlayerData>(&collection, gid).await;
                metrics.mongo_find.record(find_started.elapsed());
                let data = match loaded? {
                    Some(mut data) => {
                        normalize_player(gid, &mut data);
                        data
                    }
                    None => {
                        let data = default_player(gid);
                        let create_started = Instant::now();
                        let created = save_model(&collection, &data).await;
                        metrics.mongo_create.record(create_started.elapsed());
                        created?;
                        data
                    }
                };
                Ok(PlayerState::new(data))
            }
        },
        move |player: SavePlayer<PlayerState>| {
            let collection = collection.clone();
            async move {
                let data = player.with(|state| state.data.clone());
                save_model(&collection, &data).await?;
                player.with_mut(|state| state.dirty = false);
                Ok(())
            }
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn register_handlers(
    rpc: &RpcManager,
    frame: FrameHandle,
    redis: xframe::xredis::Client,
    runtime: LogicRuntime<PlayerState, PlayerError>,
    logic_id: i32,
    online_count: Arc<AtomicI32>,
    rpc_timeout: Duration,
    login_metrics: Arc<LoginMetrics>,
) -> xframe::xrpc::Result<()> {
    let login_runtime = runtime.clone();
    let login_frame = frame.clone();
    let login_redis = redis.clone();
    let login_online = online_count.clone();
    let login_stats = login_metrics;
    rpc.register::<pb::LogicLoginReq, _, _>(move |ctx, request| {
            let runtime = login_runtime.clone();
            let frame = login_frame.clone();
            let redis = login_redis.clone();
            let online_count = login_online.clone();
            let metrics = login_stats.clone();
            async move {
                let total_started = Instant::now();
                if request.gid <= 0
                    || request.gate_id <= 0
                    || request.player_session <= 0
                    || ctx.head.gid != request.gid as u64
                    || ctx.head.player_session != request.player_session as u64
                {
                    return Ok(pb::LogicLoginRsp {
                        status: Some(error_status(code::INVALID_ARGUMENT, "invalid Logic login")),
                        ..Default::default()
                    });
                }
                let gid = request.gid;
                let new_gate = request.gate_id;
                let new_session = request.player_session;
                let reconnect = request.reconnect;
                let kick_frame = frame.clone();
                let runtime_started = Instant::now();
                let call = runtime.try_use_preloaded(
                    gid,
                    retained_kib(&request),
                    move |player| {
                        let old = player.session;
                        async move {
                            if !reconnect
                                && let Some(old) = old
                                && (old.gate_id != new_gate || old.session_id != new_session)
                            {
                                let kick = pb::KickSessionReq {
                                    gid,
                                    player_session: old.session_id,
                                    code: code::SESSION_REPLACED,
                                    reason: "session replaced".to_string(),
                                };
                                if let Err(error) = kick_frame
                                    .call_player_to(
                                        ServiceType::Gate,
                                        old.gate_id,
                                        gid,
                                        old.session_id,
                                        &kick,
                                        rpc_timeout,
                                    )
                                    .await
                                {
                                    tracing::debug!(gid, old_gate = old.gate_id, %error, "Logic old Gate kick failed");
                                }
                            }
                            Ok(old)
                        }
                    },
                    move |player, old| {
                        let mut result = player.login(new_gate, new_session);
                        if let Some(old) = old {
                            result.response.old_gate_id = old.gate_id;
                            result.response.old_player_session = old.session_id;
                        }
                        result.response.logic_id = logic_id;
                        result
                    },
                );
                let result = await_logic(call).await;
                metrics.runtime_wait.record(runtime_started.elapsed());
                let result = match result {
                    Ok(result) => result,
                    Err(status) => {
                        return Ok(pb::LogicLoginRsp {
                            status: Some(status),
                            ..Default::default()
                        });
                    }
                };
                if result.became_online {
                    online_count.fetch_add(1, Ordering::AcqRel);
                }
                let owner_started = Instant::now();
                let owner_result = set_logic_owner(&redis, gid, logic_id).await;
                metrics.redis_owner.record(owner_started.elapsed());
                if let Err(error) = owner_result {
                    tracing::error!(gid, logic_id, %error, "Logic Redis owner save failed");
                    if let Ok(call) = runtime.try_use(gid, 1, move |player| {
                        player.disconnect(new_gate, new_session)
                    }) && await_logic(Ok(call)).await.unwrap_or(false)
                    {
                        decrement_online(&online_count);
                    }
                    return Ok(pb::LogicLoginRsp {
                        status: Some(error_status(code::INTERNAL, "Logic owner save failed")),
                        ..Default::default()
                    });
                }
                metrics.total.record(total_started.elapsed());
                Ok(result.response)
            }
        })?;

    let disconnect_runtime = runtime.clone();
    let disconnect_online = online_count.clone();
    rpc.register_notification::<pb::LogicDisconnectNtf, _, _>(move |_ctx, request| {
        let runtime = disconnect_runtime.clone();
        let online_count = disconnect_online.clone();
        async move {
            if request.gid <= 0 || request.gate_id <= 0 || request.player_session <= 0 {
                return Ok(());
            }
            if let Ok(call) = runtime.try_use(request.gid, 1, move |player| {
                player.disconnect(request.gate_id, request.player_session)
            }) && await_logic(Ok(call)).await.unwrap_or(false)
            {
                decrement_online(&online_count);
            }
            Ok(())
        }
    })?;

    let player_runtime = runtime.clone();
    rpc.register::<pb::PlayerInfoReq, _, _>(move |ctx, _request| {
        let runtime = player_runtime.clone();
        async move {
            let Some(gid) = valid_context_gid(&ctx) else {
                return Ok(pb::PlayerInfoRsp {
                    status: Some(error_status(code::INVALID_ARGUMENT, "missing player route")),
                    ..Default::default()
                });
            };
            Ok(
                match await_logic(runtime.try_use(gid, 1, |player| player.player_info())).await {
                    Ok(response) => response,
                    Err(status) => pb::PlayerInfoRsp {
                        status: Some(status),
                        ..Default::default()
                    },
                },
            )
        }
    })?;

    let use_runtime = runtime.clone();
    rpc.register::<pb::UseItemReq, _, _>(move |ctx, request| {
        let runtime = use_runtime.clone();
        async move {
            let Some(gid) = valid_context_gid(&ctx) else {
                return Ok(pb::UseItemRsp {
                    status: Some(error_status(code::INVALID_ARGUMENT, "missing player route")),
                    ..Default::default()
                });
            };
            Ok(
                match await_logic(runtime.try_use(gid, retained_kib(&request), move |player| {
                    player.use_item(request)
                }))
                .await
                {
                    Ok(response) => response,
                    Err(status) => pb::UseItemRsp {
                        status: Some(status),
                        ..Default::default()
                    },
                },
            )
        }
    })?;

    register_item_handlers(rpc, runtime)?;
    Ok(())
}

fn register_item_handlers(
    rpc: &RpcManager,
    runtime: LogicRuntime<PlayerState, PlayerError>,
) -> xframe::xrpc::Result<()> {
    let add_runtime = runtime.clone();
    rpc.register::<pb::AddItemsReq, _, _>(move |ctx, request| {
        let runtime = add_runtime.clone();
        async move {
            let gid = request.gid;
            if gid <= 0 || (ctx.head.gid != 0 && ctx.head.gid != gid as u64) {
                return Ok(pb::AddItemsRsp {
                    status: Some(error_status(
                        code::INVALID_ARGUMENT,
                        "invalid add-items route",
                    )),
                    items: Vec::new(),
                });
            }
            let retained = retained_kib(&request);
            let items = request.items;
            Ok(
                match await_logic(
                    runtime.try_use(gid, retained, move |player| player.add_items(items)),
                )
                .await
                {
                    Ok(response) => response,
                    Err(status) => pb::AddItemsRsp {
                        status: Some(status),
                        items: Vec::new(),
                    },
                },
            )
        }
    })?;

    let remove_runtime = runtime.clone();
    rpc.register::<pb::RemoveItemsReq, _, _>(move |ctx, request| {
        let runtime = remove_runtime.clone();
        async move {
            let gid = request.gid;
            if gid <= 0 || (ctx.head.gid != 0 && ctx.head.gid != gid as u64) {
                return Ok(pb::RemoveItemsRsp {
                    status: Some(error_status(
                        code::INVALID_ARGUMENT,
                        "invalid remove-items route",
                    )),
                    items: Vec::new(),
                });
            }
            let retained = retained_kib(&request);
            let items = request.items;
            Ok(
                match await_logic(
                    runtime.try_use(gid, retained, move |player| player.remove_items(items)),
                )
                .await
                {
                    Ok(response) => response,
                    Err(status) => pb::RemoveItemsRsp {
                        status: Some(status),
                        items: Vec::new(),
                    },
                },
            )
        }
    })?;

    let check_runtime = runtime;
    rpc.register::<pb::CheckItemsReq, _, _>(move |ctx, request| {
        let runtime = check_runtime.clone();
        async move {
            let gid = request.gid;
            if gid <= 0 || (ctx.head.gid != 0 && ctx.head.gid != gid as u64) {
                return Ok(pb::CheckItemsRsp {
                    status: Some(error_status(
                        code::INVALID_ARGUMENT,
                        "invalid check-items route",
                    )),
                    enough: false,
                });
            }
            let retained = retained_kib(&request);
            let items = request.items;
            Ok(
                match await_logic(
                    runtime.try_use(gid, retained, move |player| player.check_items(items)),
                )
                .await
                {
                    Ok(response) => response,
                    Err(status) => pb::CheckItemsRsp {
                        status: Some(status),
                        enough: false,
                    },
                },
            )
        }
    })?;
    Ok(())
}

async fn await_logic<R>(
    call: Result<LogicCall<R, PlayerError>, RejectReason>,
) -> Result<R, pb::Status> {
    let call = call.map_err(reject_status)?;
    let Completed { value, persistence } = call.await.map_err(call_status)?;
    persistence.map_err(|error| error_status(code::INTERNAL, error.to_string()))?;
    Ok(value)
}

fn reject_status(reason: RejectReason) -> pb::Status {
    let message = match reason {
        RejectReason::Calls => "Logic call capacity exhausted",
        RejectReason::KiB => "Logic retained bytes exhausted",
        RejectReason::Gid => "Logic player mailbox exhausted",
        RejectReason::Draining => "Logic is draining",
    };
    error_status(code::OVERLOADED, message)
}

fn call_status(error: LogicCallError<PlayerError>) -> pb::Status {
    match error {
        LogicCallError::DirtyCapacity => {
            error_status(code::OVERLOADED, "Logic dirty-player capacity exhausted")
        }
        LogicCallError::RuntimeStopped => {
            error_status(code::TEMPORARILY_UNAVAILABLE, "Logic stopped")
        }
        other => error_status(code::INTERNAL, other.to_string()),
    }
}

fn valid_context_gid(ctx: &xframe::xrpc::RpcContext) -> Option<i64> {
    let gid = i64::try_from(ctx.head.gid).ok()?;
    (gid > 0 && ctx.head.player_session > 0).then_some(gid)
}

fn retained_kib(message: &impl Message) -> usize {
    message.encoded_len().div_ceil(KIB).max(1)
}

fn aggregate_items(items: Vec<pb::Item>) -> HashMap<i32, i64> {
    let mut aggregate = HashMap::with_capacity(items.len());
    for item in items {
        *aggregate.entry(item.item_id).or_default() += item.count;
    }
    aggregate
}

fn default_player(gid: i64) -> pb::PlayerData {
    pb::PlayerData {
        gid,
        profile: Some(pb::PlayerInfo {
            gid,
            name: format!("Player{gid}"),
            level: 1,
            icon: 0,
            exp: 0,
        }),
        items: HashMap::new(),
    }
}

fn normalize_player(gid: i64, data: &mut pb::PlayerData) {
    data.gid = gid;
    let profile = data
        .profile
        .get_or_insert_with(|| default_player(gid).profile.unwrap());
    profile.gid = gid;
}

fn decrement_online(online_count: &AtomicI32) {
    let _ = online_count.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some((current - 1).max(0))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_changes_are_aggregated_before_mutation() {
        let mut player = PlayerState::new(default_player(7));
        let response = player.add_items(vec![
            pb::Item {
                item_id: 1,
                count: 2,
                change: 0,
            },
            pb::Item {
                item_id: 1,
                count: 3,
                change: 0,
            },
        ]);
        assert_eq!(response.status.unwrap().code, code::OK);
        assert_eq!(player.data.items[&1], 5);
        assert!(player.dirty);
    }

    #[test]
    fn remove_items_is_all_or_nothing() {
        let mut player = PlayerState::new(default_player(7));
        player.data.items.insert(1, 5);
        let response = player.remove_items(vec![
            pb::Item {
                item_id: 1,
                count: 3,
                change: 0,
            },
            pb::Item {
                item_id: 2,
                count: 1,
                change: 0,
            },
        ]);
        assert_eq!(response.status.unwrap().code, code::INSUFFICIENT_ITEMS);
        assert_eq!(player.data.items[&1], 5);
        assert!(!player.dirty);
    }
}
