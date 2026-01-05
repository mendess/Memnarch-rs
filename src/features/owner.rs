use serenity::all::{CacheHttp, CreateMessage, UserId};

pub async fn notify_owner(cache_http: impl CacheHttp, message: String) -> serenity::Result<()> {
    const OWNER: UserId = UserId::new(98500250540478464);

    OWNER
        .dm(cache_http, CreateMessage::new().content(message))
        .await?;
    Ok(())
}
