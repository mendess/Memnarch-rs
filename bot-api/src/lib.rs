use std::{fmt, sync::Arc};

use actix_web::{
    web::{self, Data},
    App, HttpResponse, HttpServer, Responder, ResponseError,
};
use serde::{Deserialize, Serialize};
use serenity::{model::id::UserId, CacheAndHttp};

#[derive(Serialize, Deserialize)]
struct Dm {
    user_id: UserId,
    body: MessageBody,
}

#[derive(Serialize, Deserialize)]
enum MessageBody {
    Text(String),
    Embed {
        title: String,
        fields: Vec<(String, String)>,
        #[serde(default)]
        img: Option<String>,
        #[serde(default)]
        url: Option<String>,
    },
}

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
    req: web::Json<Dm>,
) -> Result<impl Responder, Error> {
    req.user_id
        .create_dm_channel(&*cache_http)
        .await?
        .send_message(&cache_http.http, |dm| match &req.body {
            MessageBody::Text(content) => dm.content(content),
            MessageBody::Embed {
                title,
                fields,
                img,
                url,
            } => dm.embed(|e| {
                if let Some(img) = img {
                    e.image(img);
                }
                if let Some(url) = url {
                    e.url(url);
                }
                e.title(title)
                    .fields(fields.iter().map(|(t, c)| (t, c, true)))
            }),
        })
        .await?;

    Ok(HttpResponse::Ok().finish())
}

pub async fn start(c: Arc<CacheAndHttp>) -> std::io::Result<()> {
    let cache_http = Data::from(c);
    println!(
        "{}",
        serde_json::to_string_pretty(&Dm {
            user_id: 123.into(),
            body: MessageBody::Text("aaaaaaa".into())
        })
        .unwrap()
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&Dm {
            user_id: 123.into(),
            body: MessageBody::Embed {
                title: "title".into(),
                fields: vec![("t".into(), "c".into())],
                img: Some("url".into()),
                url: Some("url".into()),
            }
        })
        .unwrap()
    );
    let server = HttpServer::new(move || {
        App::new()
            .app_data(cache_http.clone())
            .route("/send-dm", web::post().to(send_dm))
    })
    .bind(("127.0.0.1", 8080))?
    .run();

    server.await
}
