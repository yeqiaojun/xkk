use xframe::xmongo::{
    self, BsonPathGetter, Collection,
    mongodb::bson::{Bson, Document, doc},
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
