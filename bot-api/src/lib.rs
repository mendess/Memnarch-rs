pub use bot_api_types as request_types;

use std::{fmt, sync::Arc};

use actix_web::{
    web::{self, Data},
    App, HttpResponse, HttpServer, Responder, ResponseError,
};
use request_types::{Dm, MessageBody};
use serenity::{model::id::UserId, CacheAndHttp};

#[derive(Debug)]
enum Error {
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
        }
    }
}

impl ResponseError for Error {
    fn status_code(&self) -> reqwest::StatusCode {
        match self {
            Error::Serenity(_) => reqwest::StatusCode::BAD_REQUEST,
        }
    }
}

async fn send_dm(
    cache_http: Data<CacheAndHttp>,
    mut req: web::Json<Dm>,
) -> Result<impl Responder, Error> {
    UserId(req.user_id)
        .create_dm_channel(&*cache_http)
        .await?
        .send_message(&cache_http.http, |dm| match &mut req.body {
            MessageBody::Text(content) => dm.content(content),
            MessageBody::Embed(bot_api_types::Embed {
                title,
                fields,
                img,
                url,
                thumbnail,
            }) => dm.embed(|e| {
                if let Some(img) = img {
                    e.image(img);
                }
                if let Some(url) = url {
                    e.url(url);
                }
                if let Some(thumbnail) = thumbnail {
                    e.thumbnail(thumbnail);
                }
                e.title(title).fields(fields.drain(..))
            }),
        })
        .await?;

    Ok(HttpResponse::Ok().finish())
}

pub async fn start(c: Arc<CacheAndHttp>) -> std::io::Result<()> {
    let cache_http = Data::from(c);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(cache_http.clone())
            .route("/send-dm", web::post().to(send_dm))
    })
    .bind(("0.0.0.0", 8080))?
    .run();

    server.await
}
