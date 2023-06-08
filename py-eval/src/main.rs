use actix_web::{http::StatusCode, web, App, HttpResponse, HttpServer, ResponseError};
use core::fmt;
use pyo3::{
    types::{PyDict, PyFloat, PyFunction, PyInt, PyList, PyString, PyTuple},
    PyAny, Python,
};
use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::{task::spawn_blocking, time::timeout};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    if env::var_os("RUST_LOG").is_none() {
        env::set_var("RUST_LOG", "info");
    }
    tracing_subscriber::fmt::init();

    let addr = SocketAddr::from((
        std::env::args()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(Ipv4Addr::LOCALHOST),
        31415,
    ));
    info!("Listening on {}", addr);
    HttpServer::new(|| {
        App::new()
            .route("/", web::post().to(eval))
            .route("/expr", web::post().to(expr))
    })
    .bind(addr)
    .unwrap()
    .run()
    .await
    .unwrap()
}

#[derive(serde::Deserialize, Debug)]
struct Program {
    t: String,
}

#[derive(Debug)]
enum Error {
    FailedToSerializeReponse(anyhow::Error),
    Unexpected(&'static str),
    FailedToRun(pyo3::PyErr),
    Timeout(Duration),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::FailedToSerializeReponse(e) => write!(f, "failed to serialize response: {e}"),
            Error::Unexpected(msg) => write!(f, "{msg}"),
            Error::FailedToRun(e) => write!(f, "python execution failed: {e}"),
            Error::Timeout(dur) => write!(f, "runtime timeout exceded: {dur:?}"),
        }
    }
}

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        match self {
            Error::FailedToSerializeReponse(_) => StatusCode::BAD_REQUEST,
            Error::Unexpected(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::FailedToRun(_) => StatusCode::BAD_REQUEST,
            Error::Timeout(_) => StatusCode::REQUEST_TIMEOUT,
        }
    }
}

#[tracing::instrument]
async fn eval(json_program: web::Json<Program>) -> Result<HttpResponse, Error> {
    let web::Json(Program { t }) = json_program;
    const TIMEOUT: Duration = Duration::from_secs(2);
    let timeout = timeout(
        TIMEOUT,
        spawn_blocking(move || {
            let py = Python::acquire_gil();
            let py = py.python();
            let locals = PyDict::new(py);
            match py.run(&t, None, Some(locals)) {
                Ok(_) => serialize_into_response(locals),
                Err(e) => {
                    error!("Failed to run: {:?}", e);
                    Err(Error::FailedToRun(e))
                }
            }
        }),
    )
    .await;

    match timeout {
        Ok(Ok(r)) => Ok(match r? {
            Some(json) => HttpResponse::Ok().json(json),
            None => HttpResponse::NoContent().json(serde_json::Value::Null),
        }),
        Ok(Err(e)) => {
            error!("Error joining: {:?}", e);
            Err(Error::Unexpected("Someone did a fucky wuky"))
        }
        Err(_) => Err(Error::Timeout(TIMEOUT)),
    }
}

#[tracing::instrument]
async fn expr(json_program: web::Json<Program>) -> Result<HttpResponse, Error> {
    let web::Json(Program { t }) = json_program;

    const TIMEOUT: Duration = Duration::from_secs(2);

    let timeout = timeout(
        TIMEOUT,
        spawn_blocking(move || {
            let py = Python::acquire_gil();
            let py = py.python();
            let locals = PyDict::new(py);
            match py.eval(&t, None, Some(locals)) {
                Ok(obj) => serialize_into_response(obj),
                Err(e) => {
                    error!("Failed to eval: {:?}", e);
                    Err(Error::FailedToRun(e))
                }
            }
        }),
    )
    .await;

    match timeout {
        Ok(Ok(r)) => Ok(match r? {
            Some(json) => HttpResponse::Ok().json(json),
            None => HttpResponse::NoContent().json(serde_json::Value::Null),
        }),
        Ok(Err(e)) => {
            error!("Error joining: {:?}", e);
            Err(Error::Unexpected("Someone did a fucky wuky"))
        }
        Err(_) => Err(Error::Timeout(TIMEOUT)),
    }
}

fn serialize_into_response(obj: &PyAny) -> Result<Option<serde_json::Value>, Error> {
    match serialize(obj) {
        Ok(Some(json)) => Ok(Some(json)),
        Ok(None) => Ok(None),
        Err(e) => {
            error!("Failed to serialize: {:?}", e);
            Err(Error::FailedToSerializeReponse(e))
        }
    }
}

fn serialize(obj: &PyAny) -> anyhow::Result<Option<serde_json::Value>> {
    use serde_json::{Number, Value};

    let r = match obj {
        o if o.is_none() => Value::Null,
        o if o.is_instance::<PyList>()? => Value::Array(
            o.cast_as::<PyList>()
                .unwrap()
                .iter()
                .map(serialize)
                .filter_map(|x| x.transpose())
                .collect::<Result<Vec<_>, _>>()?,
        ),
        o if o.is_instance::<PyTuple>()? => Value::Array(
            o.cast_as::<PyTuple>()
                .unwrap()
                .iter()
                .map(serialize)
                .filter_map(|x| x.transpose())
                .collect::<Result<Vec<_>, _>>()?,
        ),
        o if o.is_instance::<PyDict>()? => Value::Object(
            o.cast_as::<PyDict>()
                .unwrap()
                .items()
                .into_iter()
                .filter_map(|x| {
                    let tup = x.cast_as::<PyTuple>().unwrap();
                    let (key, value) = (tup.get_item(0), tup.get_item(1));
                    Some(
                        serialize(value)
                            .transpose()?
                            .and_then(|v| Ok((key.str()?.to_string_lossy().into_owned(), v))),
                    )
                })
                .collect::<Result<_, _>>()?,
        ),
        o if o.is_instance::<PyInt>()? => Value::Number(Number::from(o.extract::<i64>()?)),
        o if o.is_instance::<PyFloat>()? => Number::from_f64(o.extract()?)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(o.str().unwrap().to_string_lossy().into())),
        o if o.is_instance::<PyString>()? => Value::String(o.extract()?),
        o if o.is_instance::<PyFunction>()? => return Ok(None),
        o => Value::String(o.str()?.to_string_lossy().into()),
    };
    Ok(Some(r))
}
