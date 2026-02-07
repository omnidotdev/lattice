use std::fmt;

/// Resolver for the MongoDB Atlas Data API, which is a MongoDB endpoint using
/// HTTP protocol for transfer.
///
/// # Internal documentation
/// TODO: update reference (formerly internal Notion page)
#[serde_with::minify_field_names(serialize = "minified", deserialize = "minified")]
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Hash, PartialEq, Eq)]
pub struct AtlasDataApiResolver {
    /// The type of operation to execute in the target.
    pub operation_type: OperationType,
    pub directive_name: String,
    pub collection: String,
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum OperationType {
    FindOne,
    FindMany,
    InsertOne,
    InsertMany,
    DeleteOne,
    DeleteMany,
    UpdateOne,
    UpdateMany,
}

impl AsRef<str> for OperationType {
    fn as_ref(&self) -> &str {
        match self {
            Self::FindOne => "findOne",
            Self::FindMany => "find",
            Self::InsertOne => "insertOne",
            Self::InsertMany => "insertMany",
            Self::DeleteOne => "deleteOne",
            Self::DeleteMany => "deleteMany",
            Self::UpdateOne => "updateOne",
            Self::UpdateMany => "updateMany",
        }
    }
}

impl fmt::Display for OperationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}
