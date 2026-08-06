use xframe::xmongo::{
    self, BsonPathGetter, Collection,
    mongodb::{
        bson::{Bson, Document, doc},
        options::{ReplaceOneModel, WriteModel},
    },
};

pub async fn load_model<T>(
    collection: &Collection<Document>,
    id: impl Into<Bson>,
) -> xmongo::Result<Option<T>>
where
    T: BsonPathGetter,
{
    let document = collection
        .find_one(doc! { "_id": id.into() })
        .await
        .map_err(xmongo::Error::from)?;
    document
        .map(|document| T::from_bson_value(&Bson::Document(document)))
        .transpose()
}

pub async fn save_model<T>(collection: &Collection<Document>, model: &T) -> xmongo::Result<()>
where
    T: BsonPathGetter,
{
    let Bson::Document(document) = model.bson_value()? else {
        unreachable!("xmongo generated models always encode to BSON documents")
    };
    let id = document
        .get("_id")
        .cloned()
        .ok_or_else(|| xmongo::Error::InvalidBsonPath("_id".to_string()))?;

    collection
        .replace_one(doc! { "_id": id }, document)
        .upsert(true)
        .await
        .map_err(xmongo::Error::from)?;
    Ok(())
}

pub async fn save_models<T>(
    collection: &Collection<Document>,
    models: impl IntoIterator<Item = T>,
) -> xmongo::Result<usize>
where
    T: BsonPathGetter,
{
    let namespace = collection.namespace();
    let writes = models
        .into_iter()
        .map(|model| {
            let Bson::Document(document) = model.bson_value()? else {
                unreachable!("xmongo generated models always encode to BSON documents")
            };
            let id = document
                .get("_id")
                .cloned()
                .ok_or_else(|| xmongo::Error::InvalidBsonPath("_id".to_string()))?;
            Ok::<WriteModel, xmongo::Error>(
                ReplaceOneModel::builder()
                    .namespace(namespace.clone())
                    .filter(doc! { "_id": id })
                    .replacement(document)
                    .upsert(true)
                    .build()
                    .into(),
            )
        })
        .collect::<xmongo::Result<Vec<_>>>()?;
    let count = writes.len();
    if writes.is_empty() {
        return Ok(0);
    }
    collection
        .client()
        .raw()
        .bulk_write(writes)
        .ordered(false)
        .await
        .map_err(xmongo::Error::from)?;
    Ok(count)
}
