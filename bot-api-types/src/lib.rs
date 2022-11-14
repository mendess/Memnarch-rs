use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Dm {
    pub user_id: u64,
    pub body: MessageBody,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum MessageBody {
    Text(String),
    Embed(Embed),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Embed {
    pub title: String,
    pub fields: Vec<(String, String, bool)>,
    #[serde(default)]
    pub img: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

impl Embed {
    fn new(title: String) -> Self {
        Self {
            title,
            fields: Default::default(),
            img: Default::default(),
            thumbnail: Default::default(),
            url: Default::default(),
        }
    }
}

impl MessageBody {
    pub fn build_embed(title: String) -> EmbedBuilder {
        EmbedBuilder {
            embed: Embed::new(title),
        }
    }
}

pub struct EmbedBuilder {
    embed: Embed,
}

impl EmbedBuilder {
    pub fn field<K, V>(&mut self, key: K, value: V, inline: bool) -> &mut Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.embed.fields.push((key.into(), value.into(), inline));
        self
    }

    pub fn fields<I, K, V>(&mut self, iter: I) -> &mut Self
    where
        I: Iterator<Item = (K, V, bool)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.embed
            .fields
            .extend(iter.map(|(k, v, i)| (k.into(), v.into(), i)));
        self
    }

    pub fn img<I>(&mut self, img: I) -> &mut Self
    where
        I: Into<String>,
    {
        self.embed.img = img.into().into();
        self
    }

    pub fn thumbnail<I>(&mut self, thumbnail: I) -> &mut Self
    where
        I: Into<String>,
    {
        self.embed.thumbnail = thumbnail.into().into();
        self
    }

    pub fn url<I>(&mut self, url: I) -> &mut Self
    where
        I: Into<String>,
    {
        self.embed.url = url.into().into();
        self
    }

    pub fn build(self) -> Embed {
        self.embed
    }
}
