use std::collections::HashMap;

use xframe::xmongo::{
    self, BsonPathGetter, Collection,
    mongodb::bson::{Bson, Document, doc},
};
use xkk_protocol::pb;

use crate::model::{load_model, save_model, save_models};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Mongo DSN must include a database")]
    MissingMongoDatabase,
    #[error(transparent)]
    Mongo(#[from] xmongo::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct AccountStore {
    pub(crate) collection: Collection<Document>,
}

impl AccountStore {
    pub(crate) fn new(collection: Collection<Document>) -> Self {
        Self { collection }
    }

    pub async fn load(&self, account: &str) -> Result<Option<pb::AccountData>> {
        Ok(load_model(&self.collection, account).await?)
    }

    pub async fn save(&self, account: &pb::AccountData) -> Result<()> {
        Ok(save_model(&self.collection, account).await?)
    }
}

#[derive(Clone)]
pub struct PlayerStore {
    pub(crate) collection: Collection<Document>,
}

impl PlayerStore {
    pub(crate) fn new(collection: Collection<Document>) -> Self {
        Self { collection }
    }

    pub async fn load(&self, gid: i64) -> Result<Option<pb::PlayerData>> {
        Ok(load_model(&self.collection, gid).await?)
    }

    pub async fn save(&self, player: &pb::PlayerData) -> Result<()> {
        Ok(save_model(&self.collection, player).await?)
    }

    pub async fn load_profiles(&self, gids: &[i64]) -> Result<Vec<pb::PlayerInfo>> {
        if gids.is_empty() {
            return Ok(Vec::new());
        }
        let ids = Bson::Array(gids.iter().copied().map(Bson::Int64).collect());
        let mut cursor = self
            .collection
            .find(doc! { "_id": { "$in": ids } })
            .await
            .map_err(xmongo::Error::from)?;
        let mut loaded = HashMap::with_capacity(gids.len());
        while cursor.advance().await.map_err(xmongo::Error::from)? {
            let document = cursor.deserialize_current().map_err(xmongo::Error::from)?;
            let player = pb::PlayerData::from_bson_value(&Bson::Document(document))?;
            if let Some(profile) = player.profile {
                loaded.insert(player.gid, profile);
            }
        }
        Ok(gids.iter().filter_map(|gid| loaded.remove(gid)).collect())
    }
}

#[derive(Clone)]
pub struct PublicPlayerStore {
    pub(crate) collection: Collection<Document>,
}

impl PublicPlayerStore {
    pub(crate) fn new(collection: Collection<Document>) -> Self {
        Self { collection }
    }

    pub async fn load(&self, gid: i64) -> Result<Option<pb::PublicPlayerData>> {
        Ok(load_model(&self.collection, gid).await?)
    }

    pub async fn save(&self, player: &pb::PublicPlayerData) -> Result<()> {
        Ok(save_model(&self.collection, player).await?)
    }

    pub async fn save_batch(
        &self,
        players: impl IntoIterator<Item = pb::PublicPlayerData>,
    ) -> Result<usize> {
        Ok(save_models(&self.collection, players).await?)
    }
}
