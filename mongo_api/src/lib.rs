use bson::Document;
use futures::stream::TryStreamExt;
use mongodb::{
    bson::{doc, extjson::de::Error, oid::ObjectId},
    options::UpdateModifications,
    results::{DeleteResult, InsertOneResult, UpdateResult},
    Client, Collection, Database,
};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
pub trait MongoDbModel: Sync + Send + Unpin {
    fn model_name() -> String;
}

#[derive(Clone)]
pub struct MongoDbClient {
    client: Client,
    db: Database,
}

impl MongoDbClient {
    pub async fn init(uri: &str, database: &str) -> Self {
        let client = match Client::with_uri_str(uri).await {
            Ok(connector) => connector,
            Err(_) => panic!(
                "err to connect database, please check the database running status or mongo db url"
            ),
        };
        let db = client.database(database);
        MongoDbClient { client, db }
    }

    fn get_collection<T: MongoDbModel>(&self) -> Collection<T> {
        let model_name = T::model_name().clone();
        let col = self.db.collection::<T>(&model_name);
        col
    }

    pub async fn insert_one<T>(&self, data: &T) -> Result<InsertOneResult, Error>
    where
        T: MongoDbModel + Serialize,
    {
        let col = self.get_collection::<T>();
        match col.insert_one(data, None).await {
            Ok(ret) => Ok(ret),
            Err(e) => {
                println!("error occured while inserting data: {}", e);
                panic!("error occured while inserting data")
            }
        }
    }

    pub async fn find_by_id<T>(&self, id: &str) -> Result<T, Error>
    where
        T: MongoDbModel + DeserializeOwned,
    {
        let col = self.get_collection::<T>();
        let obj_id = ObjectId::parse_str(id).expect("there is no id for find");
        let filter = doc! {"_id": obj_id};
        let ret = col
            .find_one(filter, None)
            .await
            .ok()
            .expect("Error occured while finding document");

        Ok(ret.unwrap())
    }

    pub async fn update_item<T>(&self, id: &str, new_item: T) -> Result<UpdateResult, Error>
    where
        T: MongoDbModel + Serialize + DeserializeOwned,
    {
        let col = self.get_collection::<T>();
        let obj_id = ObjectId::parse_str(id).expect("there is no id for update");
        let filter = doc! {"_id": obj_id};
        let redacted_bson = bson::to_bson(&new_item).unwrap();

        let new_doc = doc! {
            "$set": redacted_bson
        };
        let ret = col
            .update_one(filter, new_doc, None)
            .await
            .ok()
            .expect("Error updating document");
        Ok(ret)
    }

    pub async fn delete_item<T>(&self, id: &str) -> Result<DeleteResult, Error>
    where
        T: MongoDbModel + DeserializeOwned,
    {
        let col = self.get_collection::<T>();
        let obj_id = ObjectId::parse_str(id).expect("there is no id for delete");
        let filter = doc! {"_id": obj_id};
        let ret = col
            .delete_one(filter, None)
            .await
            .ok()
            .expect("Error deleting item");
        Ok(ret)
    }

    pub async fn delete_items<T>(&self, items: Vec<String>) -> Result<DeleteResult, Error>
    where
        T: MongoDbModel + DeserializeOwned,
    {
        let col = self.get_collection::<T>();
        let mut obj_ids: Vec<ObjectId> = Vec::new();
        items.iter().for_each(|v| match ObjectId::parse_str(v) {
            Ok(val) => obj_ids.push(val),
            Err(e) => println!("one of id is not correct. :{}", e),
        });

        let ids = bson::to_bson(&obj_ids).expect("Error converting items for massive delete");
        let filter = doc! {"_id": { "$in" : ids}};
        let ret = col
            .delete_many(filter, None)
            .await
            .ok()
            .expect("Error massive deleting items");

        Ok(ret)
    }

    pub async fn filter_items<T>(&self, filter: Option<Document>) -> Result<Vec<T>, Error>
    where
        T: MongoDbModel + Serialize + DeserializeOwned,
    {
        let col = self.get_collection::<T>();
        println!("{}", T::model_name());
        dbg!(&filter);
        let mut cursors = col
            .find(filter, None)
            .await
            .unwrap_or_else(|e| panic!("Error getting list of items: {}", e));

        let mut items: Vec<T> = Vec::new();
        while let Some(item) = cursors
            .try_next()
            .await
            .ok()
            .expect("Error mapping through cursor")
        {
            items.push(item);
        }
        Ok(items)
    }

    pub async fn filter_item<T>(&self, filter: Option<Document>) -> Result<Option<T>, Error>
    where
        T: MongoDbModel + Serialize + DeserializeOwned,
    {
        let col = self.get_collection::<T>();
        let mut cursors = col
            .find(filter, None)
            .await
            .ok()
            .expect("Error getting list of items");        
        while let Some(item) = cursors
            .try_next()
            .await
            .ok()
            .expect("Error mapping through cursor")
        {
            return Ok(Some(item))
        }
        Ok(None)
    }

    pub async fn update_items<T>(
        &self,
        query: Document,
        update: Option<Document>,
    ) -> Result<UpdateResult, Error>
    where
        T: MongoDbModel + Serialize + DeserializeOwned,
    {
        //not completed for all case.
        let col = self.get_collection::<T>();
        // let ids = bson::to_bson(&query).expect("Error converting items for massive delete");
        // let query = doc! {"_id": ids};
        let ret = col
            .update_many(query, update.unwrap(), None)
            .await
            .ok()
            .expect("Error massive updating");
        Ok(ret)
    }
}
