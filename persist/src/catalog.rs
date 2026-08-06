use xframe::xmongo;

use crate::{AccountStore, Error, PlayerStore, PublicPlayerStore};

const ACCOUNTS: &str = "accounts";
const PLAYERS: &str = "players";
const PUBLIC_PLAYERS: &str = "public_players";

#[derive(Clone)]
pub struct Database {
    accounts: AccountStore,
    players: PlayerStore,
    public_players: PublicPlayerStore,
}

impl Database {
    pub fn new(mongo: xmongo::Client) -> Result<Self, Error> {
        let database = mongo
            .options()
            .default_database
            .as_deref()
            .ok_or(Error::MissingMongoDatabase)?;
        Ok(Self {
            accounts: AccountStore::new(mongo.collection(database, ACCOUNTS)),
            players: PlayerStore::new(mongo.collection(database, PLAYERS)),
            public_players: PublicPlayerStore::new(mongo.collection(database, PUBLIC_PLAYERS)),
        })
    }

    pub fn accounts(&self) -> AccountStore {
        self.accounts.clone()
    }

    pub fn players(&self) -> PlayerStore {
        self.players.clone()
    }

    pub fn public_players(&self) -> PublicPlayerStore {
        self.public_players.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xframe::xmongo::mongodb::options::ClientOptions;

    #[tokio::test]
    async fn collection_catalog_uses_the_dsn_database_and_fixed_names() {
        let mut options = ClientOptions::default();
        options.default_database = Some("xkk_test".to_string());
        let mongo = xmongo::Client::with_options(options).unwrap();

        let database = Database::new(mongo).unwrap();

        assert_eq!(
            database.accounts.collection.raw().namespace().db,
            "xkk_test"
        );
        assert_eq!(
            database.accounts.collection.raw().namespace().coll,
            "accounts"
        );
        assert_eq!(
            database.players.collection.raw().namespace().coll,
            "players"
        );
        assert_eq!(
            database.public_players.collection.raw().namespace().coll,
            "public_players"
        );
    }

    #[tokio::test]
    async fn collection_catalog_rejects_a_dsn_without_a_database() {
        let mongo = xmongo::Client::with_options(ClientOptions::default()).unwrap();

        assert!(matches!(
            Database::new(mongo),
            Err(Error::MissingMongoDatabase)
        ));
    }
}
