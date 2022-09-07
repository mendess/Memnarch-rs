use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Dm {
    pub user_id: u64,
    pub body: MessageBody,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum MessageBody {
    Text(String),
    Embed {
        title: String,
        fields: Vec<(String, String)>,
        #[serde(default)]
        img: Option<String>,
        #[serde(default)]
        thumbnail: Option<String>,
        #[serde(default)]
        url: Option<String>,
    },
}
