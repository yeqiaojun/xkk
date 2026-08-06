use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use tokio::sync::Semaphore;
use xframe::xmongo::{
    BsonPathGetter, Collection,
    mongodb::bson::{Bson, Document, doc},
};
use xkk_protocol::{code, error_status, ok_status, pb};

// Query fan-out and concurrency are stable protection limits. They are kept in
// code so every deployment has the same behavior and review trail.
const MAX_GAMER_IDS: usize = 100;
const MAX_INFLIGHT_REQUESTS: usize = 1_024;

#[derive(Clone)]
pub(crate) struct QueryApi {
    players: Collection<Document>,
    inflight: Arc<Semaphore>,
}

impl QueryApi {
    pub fn new(players: Collection<Document>) -> Self {
        Self {
            players,
            inflight: Arc::new(Semaphore::new(MAX_INFLIGHT_REQUESTS)),
        }
    }

    pub fn available_request_slots(&self) -> usize {
        self.inflight.available_permits()
    }

    pub async fn gamer_info(&self, request: pb::GamerInfoReq) -> pb::GamerInfoRsp {
        let Ok(_permit) = self.inflight.clone().try_acquire_owned() else {
            tracing::error!(
                limit = MAX_INFLIGHT_REQUESTS,
                "Query inflight request hard limit exceeded"
            );
            return gamer_info_error(code::OVERLOADED, "Query request capacity exhausted");
        };
        if request.gamer_ids.len() > MAX_GAMER_IDS {
            tracing::error!(
                requested = request.gamer_ids.len(),
                limit = MAX_GAMER_IDS,
                "Query gamer id hard limit exceeded"
            );
        }
        let Some(gamer_ids) = valid_gamer_ids(request.gamer_ids) else {
            return gamer_info_error(code::INVALID_ARGUMENT, "invalid gamer ids");
        };
        let ids = Bson::Array(gamer_ids.iter().copied().map(Bson::Int64).collect());
        let mut cursor = match self.players.find(doc! { "_id": { "$in": ids } }).await {
            Ok(cursor) => cursor,
            Err(error) => {
                tracing::error!(?gamer_ids, %error, "Query player batch load failed");
                return gamer_info_error(code::INTERNAL, "player load failed");
            }
        };
        let mut loaded = HashMap::with_capacity(gamer_ids.len());
        loop {
            match cursor.advance().await {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    tracing::error!(?gamer_ids, %error, "Query player cursor failed");
                    return gamer_info_error(code::INTERNAL, "player load failed");
                }
            }
            let document = match cursor.deserialize_current() {
                Ok(document) => document,
                Err(error) => {
                    tracing::error!(?gamer_ids, %error, "Query player document decode failed");
                    return gamer_info_error(code::INTERNAL, "player decode failed");
                }
            };
            let player = match pb::PlayerData::from_bson_value(&Bson::Document(document)) {
                Ok(player) => player,
                Err(error) => {
                    tracing::error!(?gamer_ids, %error, "Query player BSON decode failed");
                    return gamer_info_error(code::INTERNAL, "player decode failed");
                }
            };
            if let Some(profile) = player.profile {
                loaded.insert(player.gid, profile);
            }
        }
        let players = gamer_ids
            .into_iter()
            .filter_map(|gid| loaded.remove(&gid))
            .collect();
        pb::GamerInfoRsp {
            status: Some(ok_status()),
            players,
        }
    }
}

fn valid_gamer_ids(gamer_ids: Vec<i64>) -> Option<Vec<i64>> {
    if gamer_ids.is_empty() || gamer_ids.len() > MAX_GAMER_IDS {
        return None;
    }
    let mut seen = HashSet::with_capacity(gamer_ids.len());
    gamer_ids
        .iter()
        .all(|gid| *gid > 0 && seen.insert(*gid))
        .then_some(gamer_ids)
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

status_response!(gamer_info_status, gamer_info_error, pb::GamerInfoRsp);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamer_ids_are_positive_unique_and_bounded() {
        assert_eq!(valid_gamer_ids(vec![2, 1]), Some(vec![2, 1]));
        assert!(valid_gamer_ids(Vec::new()).is_none());
        assert!(valid_gamer_ids(vec![1, 1]).is_none());
        assert!(valid_gamer_ids(vec![0]).is_none());
        assert!(valid_gamer_ids(vec![1; MAX_GAMER_IDS + 1]).is_none());
    }
}
