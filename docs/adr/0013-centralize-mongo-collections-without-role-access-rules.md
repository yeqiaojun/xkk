# Centralize Mongo collections without role access rules

`xkk-persist` declares every Mongo collection name and initializes a Persistence Collection Catalog from the database in the Mongo DSN. Any Service Package may obtain and modify any collection through that catalog: the module removes duplicated database naming without encoding role ownership or repository-style access restrictions, keeping ordinary database operations easy to extend.
