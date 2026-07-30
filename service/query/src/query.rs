use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use tokio::sync::Semaphore;
use xframe::xmongo::{
    self, BsonPathGetter, Collection,
    mongodb::bson::{Bson, Document, doc},
};
use xkk_persist::{load_model, save_model};
use xkk_protocol::{code, error_status, ok_status, pb};

#[derive(Clone)]
pub(crate) struct QueryApi {
    players: Collection<Document>,
    manifests: Collection<Document>,
    manifest: pb::ConfigManifestData,
    max_gamer_ids: usize,
    inflight: Arc<Semaphore>,
}

impl QueryApi {
    pub fn new(
        mongo: xmongo::Client,
        database: &str,
        player_collection: &str,
        manifest_collection: &str,
        manifest: pb::ConfigManifestData,
        max_gamer_ids: usize,
        max_inflight_requests: usize,
    ) -> Self {
        assert!(max_gamer_ids > 0, "Query gamer id limit must be positive");
        assert!(
            max_inflight_requests > 0,
            "Query inflight capacity must be positive"
        );
        Self {
            players: mongo.collection(database, player_collection),
            manifests: mongo.collection(database, manifest_collection),
            manifest,
            max_gamer_ids,
            inflight: Arc::new(Semaphore::new(max_inflight_requests)),
        }
    }

    pub async fn initialize(&self) -> xmongo::Result<()> {
        if load_model::<pb::ConfigManifestData>(&self.manifests, &self.manifest.version)
            .await?
            .is_none()
        {
            save_model(&self.manifests, &self.manifest).await?;
        }
        Ok(())
    }

    pub fn available_request_slots(&self) -> usize {
        self.inflight.available_permits()
    }

    pub async fn gamer_info(&self, request: pb::GamerInfoReq) -> pb::GamerInfoRsp {
        let Ok(_permit) = self.inflight.clone().try_acquire_owned() else {
            return gamer_info_error(code::OVERLOADED, "Query request capacity exhausted");
        };
        let Some(gamer_ids) = valid_gamer_ids(request.gamer_ids, self.max_gamer_ids) else {
            return gamer_info_error(code::INVALID_ARGUMENT, "invalid gamer ids");
        };
        let ids = Bson::Array(gamer_ids.iter().copied().map(Bson::Int64).collect());
        let mut cursor = match self.players.find(doc! { "_id": { "$in": ids } }).await {
            Ok(cursor) => cursor,
            Err(error) => {
                xlog::error!(?gamer_ids, %error, "Query player batch load failed");
                return gamer_info_error(code::INTERNAL, "player load failed");
            }
        };
        let mut loaded = HashMap::with_capacity(gamer_ids.len());
        loop {
            match cursor.advance().await {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    xlog::error!(?gamer_ids, %error, "Query player cursor failed");
                    return gamer_info_error(code::INTERNAL, "player load failed");
                }
            }
            let document = match cursor.deserialize_current() {
                Ok(document) => document,
                Err(error) => {
                    xlog::error!(?gamer_ids, %error, "Query player document decode failed");
                    return gamer_info_error(code::INTERNAL, "player decode failed");
                }
            };
            let player = match pb::PlayerData::from_bson_value(&Bson::Document(document)) {
                Ok(player) => player,
                Err(error) => {
                    xlog::error!(?gamer_ids, %error, "Query player BSON decode failed");
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

    pub async fn config_key(&self, _request: pb::ConfigKeyReq) -> pb::ConfigKeyRsp {
        let Ok(_permit) = self.inflight.clone().try_acquire_owned() else {
            return config_key_error(code::OVERLOADED, "Query request capacity exhausted");
        };
        match self.load_manifest().await {
            Ok(manifest) => pb::ConfigKeyRsp {
                status: Some(ok_status()),
                key: manifest.key,
            },
            Err(status) => config_key_status(status),
        }
    }

    pub async fn config_manifest(&self, request: pb::ConfigManifestReq) -> pb::ConfigManifestRsp {
        let Ok(_permit) = self.inflight.clone().try_acquire_owned() else {
            return config_manifest_error(code::OVERLOADED, "Query request capacity exhausted");
        };
        let manifest = match self.load_manifest().await {
            Ok(manifest) => manifest,
            Err(status) => return config_manifest_status(status),
        };
        let files = if request.version == manifest.version {
            Vec::new()
        } else {
            manifest.files
        };
        pb::ConfigManifestRsp {
            status: Some(ok_status()),
            version: manifest.version,
            base_url: manifest.base_url,
            files,
        }
    }

    async fn load_manifest(&self) -> Result<pb::ConfigManifestData, pb::Status> {
        match load_model(&self.manifests, &self.manifest.version).await {
            Ok(Some(manifest)) => Ok(manifest),
            Ok(None) => Err(error_status(code::NOT_FOUND, "config manifest not found")),
            Err(error) => {
                xlog::error!(version = %self.manifest.version, %error, "Query manifest load failed");
                Err(error_status(code::INTERNAL, "config manifest load failed"))
            }
        }
    }
}

fn valid_gamer_ids(gamer_ids: Vec<i64>, limit: usize) -> Option<Vec<i64>> {
    if gamer_ids.is_empty() || gamer_ids.len() > limit {
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
status_response!(config_key_status, config_key_error, pb::ConfigKeyRsp);
status_response!(
    config_manifest_status,
    config_manifest_error,
    pb::ConfigManifestRsp
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamer_ids_are_positive_unique_and_bounded() {
        assert_eq!(valid_gamer_ids(vec![2, 1], 2), Some(vec![2, 1]));
        assert!(valid_gamer_ids(Vec::new(), 2).is_none());
        assert!(valid_gamer_ids(vec![1, 1], 2).is_none());
        assert!(valid_gamer_ids(vec![0], 2).is_none());
        assert!(valid_gamer_ids(vec![1, 2, 3], 2).is_none());
    }
}
