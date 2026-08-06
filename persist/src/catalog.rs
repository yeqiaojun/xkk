use xframe::xmongo::{self, Collection, mongodb::bson::Document};

const ACCOUNTS: &str = "accounts";
const PLAYERS: &str = "players";
const PUBLIC_PLAYERS: &str = "public_players";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Mongo DSN must include a database")]
    MissingMongoDatabase,
}

#[derive(Clone)]
pub struct Collections {
    accounts: Collection<Document>,
    players: Collection<Document>,
    public_players: Collection<Document>,
}

impl Collections {
    pub fn new(mongo: xmongo::Client) -> Result<Self, Error> {
        let database = mongo
            .options()
            .default_database
            .as_deref()
            .ok_or(Error::MissingMongoDatabase)?;
        Ok(Self {
            accounts: mongo.collection(database, ACCOUNTS),
            players: mongo.collection(database, PLAYERS),
            public_players: mongo.collection(database, PUBLIC_PLAYERS),
        })
    }

    pub fn accounts(&self) -> Collection<Document> {
        self.accounts.clone()
    }

    pub fn players(&self) -> Collection<Document> {
        self.players.clone()
    }

    pub fn public_players(&self) -> Collection<Document> {
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

        let collections = Collections::new(mongo).unwrap();

        assert_eq!(collections.accounts().raw().namespace().db, "xkk_test");
        assert_eq!(collections.accounts().raw().namespace().coll, "accounts");
        assert_eq!(collections.players().raw().namespace().coll, "players");
        assert_eq!(
            collections.public_players().raw().namespace().coll,
            "public_players"
        );
    }

    #[tokio::test]
    async fn collection_catalog_rejects_a_dsn_without_a_database() {
        let mongo = xmongo::Client::with_options(ClientOptions::default()).unwrap();

        assert!(matches!(
            Collections::new(mongo),
            Err(Error::MissingMongoDatabase)
        ));
    }
}
