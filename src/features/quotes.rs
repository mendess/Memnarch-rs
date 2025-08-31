use crate::in_files;
use rand::seq::SliceRandom as _;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::{
    fs::{DirBuilder, File},
    io::{AsyncReadExt as _, AsyncWriteExt as _},
};

const QUOTES_DIR: &str = "quotes";
const QUOTES_FILE: &str = "quotes.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuoteManager(Vec<String>);

impl QuoteManager {
    async fn path() -> std::io::Result<PathBuf> {
        let p = PathBuf::from(in_files!(QUOTES_DIR, QUOTES_FILE));
        DirBuilder::new()
            .recursive(true)
            .create(p.parent().expect("This path always has enough components"))
            .await?;
        Ok(p)
    }

    pub(crate) async fn load() -> std::io::Result<Self> {
        let path = Self::path().await?;
        let mut file = File::open(path).await?;
        let mut s = String::new();
        file.read_to_string(&mut s).await.and_then(|_| {
            serde_json::from_str(&s).map_err(|e| {
                tracing::error!("Error parsing quotes");
                e.into()
            })
        })
    }

    pub fn choose(&self) -> Option<&str> {
        self.0.choose(&mut rand::thread_rng()).map(|x| x.as_str())
    }

    pub async fn add(&mut self, quote: String) -> std::io::Result<()> {
        self.0.push(quote);
        let path = Self::path().await?;
        tracing::trace!("Quote add: {:?}", path);
        let content = serde_json::to_string(self)?;
        File::create(path)
            .await?
            .write_all(content.as_bytes())
            .await
    }
}
