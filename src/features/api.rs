pub use bot_api_types as request_types;

use std::{fmt, sync::Arc};

use actix_web::{
    App, HttpResponse, HttpServer, Responder, ResponseError,
    web::{self, Data},
};
use request_types::{Dm, MessageBody};
use serenity::all::{Cache, ChannelId, CreateEmbed, CreateMessage, Http};

#[derive(Debug)]
enum Error {
    UrlParseError(String),
    Serenity(String),
}

impl From<serenity::Error> for Error {
    fn from(e: serenity::Error) -> Self {
        Error::Serenity(e.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Serenity(e) => write!(f, "serenity error: {e}"),
            Error::UrlParseError(e) => write!(f, "url parse error: {e}"),
        }
    }
}

impl ResponseError for Error {
    fn status_code(&self) -> reqwest::StatusCode {
        match self {
            Error::UrlParseError(_) | Error::Serenity(_) => reqwest::StatusCode::BAD_REQUEST,
        }
    }
}

type CacheAndHttp = (Arc<Cache>, Arc<Http>);

async fn send_dm(
    cache_http: Data<CacheAndHttp>,
    req: web::Json<Dm>,
) -> Result<impl Responder, Error> {
    let (cache, http) = cache_http.get_ref();
    req.user_id
        .create_dm_channel((cache, &**http))
        .await?
        .send_message(
            (cache, &**http),
            match req.into_inner().body {
                MessageBody::Text(content) => CreateMessage::new().content(content),
                MessageBody::Embed(bot_api_types::Embed {
                    title,
                    mut fields,
                    img,
                    url,
                    thumbnail,
                }) => CreateMessage::new().embed({
                    let mut e = CreateEmbed::new();
                    if let Some(img) = img {
                        e = e.image(img);
                    }
                    if let Some(url) = url {
                        e = e.url(url);
                    }
                    if let Some(thumbnail) = thumbnail {
                        e = e.thumbnail(thumbnail);
                    }
                    e.title(title).fields(fields.drain(..))
                }),
            },
        )
        .await?;

    Ok(HttpResponse::Ok().finish())
}

async fn send_song(
    cache_http: Data<CacheAndHttp>,
    web::Json(request_types::Banger {
        author,
        channel_id,
        url,
    }): web::Json<request_types::Banger>,
) -> Result<impl Responder, Error> {
    let url = url
        .parse::<reqwest::Url>()
        .map_err(|e| Error::UrlParseError(e.to_string()))?;
    crate::features::music_channel_broadcast::send_to(
        (&cache_http.0, &*cache_http.1),
        author,
        ChannelId::new(1414964946416177333),
        &url,
        channel_id,
        crate::features::music_channel_broadcast::SpotifyScrape::of(&url)
            .await
            .as_ref(),
    )
    .await
    .map_err(|e| Error::Serenity(e.to_string()))?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn start(c: CacheAndHttp) -> std::io::Result<()> {
    let cache_http = Data::new(c);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(cache_http.clone())
            .route("/send-dm", web::post().to(send_dm))
            .route("/rattlesnake_burrow", web::post().to(send_song))
    })
    .bind(("0.0.0.0", 8080))?
    .run();

    server.await
}
