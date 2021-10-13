use std::time::Duration;

use axum::{handler::post, http::StatusCode, Json, Router};
use pyo3::{
    types::{PyDict, PyFloat, PyFunction, PyInt, PyList, PyString, PyTuple},
    PyAny, Python,
};
use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
};
use tokio::{task::spawn_blocking, time::timeout};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    if env::var_os("RUST_LOG").is_none() {
        env::set_var("RUST_LOG", "info");
    }
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", post(eval))
        .route("/expr", post(expr));

    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 31415));
    info!("Listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

#[derive(serde::Deserialize, Debug)]
struct Program {
    t: String,
}

type Response<T> = (StatusCode, T);
type HttpResponse = Result<Response<Json<serde_json::Value>>, Response<String>>;

#[tracing::instrument]
async fn eval(json_program: Json<Program>) -> HttpResponse {
    let Json(Program { t }) = json_program;
    let timeout = timeout(
        Duration::from_secs(10),
        spawn_blocking(move || {
            let py = Python::acquire_gil();
            let py = py.python();
            let locals = PyDict::new(py);
            match py.run(&t, None, Some(locals)) {
                Ok(_) => serialize_into_response(&locals),
                Err(e) => {
                    error!("Failed to run: {:?}", e);
                    Err((StatusCode::BAD_REQUEST, format!("{:?}", e)))
                }
            }
        }),
    )
    .await;

    match timeout {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            error!("Error joining: {:?}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                String::from("Someone did a fucky wuky"),
            ))
        }
        Err(_) => Err((
            StatusCode::REQUEST_TIMEOUT,
            String::from("Can't take more than 10 seconds"),
        )),
    }
}

#[tracing::instrument]
async fn expr(json_program: Json<Program>) -> HttpResponse {
    let Json(Program { t }) = json_program;

    let timeout = timeout(
        Duration::from_secs(2),
        spawn_blocking(move || {
            let py = Python::acquire_gil();
            let py = py.python();
            let locals = PyDict::new(py);
            match py.eval(&t, None, Some(locals)) {
                Ok(obj) => serialize_into_response(&obj),
                Err(e) => {
                    error!("Failed to eval: {:?}", e);
                    Err((StatusCode::BAD_REQUEST, format!("{:?}", e)))
                }
            }
        }),
    )
    .await;

    match timeout {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            error!("Error joining: {:?}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                String::from("Someone did a fucky wuky"),
            ))
        }
        Err(_) => Err((
            StatusCode::REQUEST_TIMEOUT,
            String::from("Can't take more than 2 seconds"),
        )),
    }
}

fn serialize_into_response(obj: &PyAny) -> HttpResponse {
    match serialize(obj) {
        Ok(Some(json)) => Ok((StatusCode::OK, Json(json))),
        Ok(None) => Ok((StatusCode::NO_CONTENT, Json(serde_json::Value::Null))),
        Err(e) => {
            error!("Failed to serialize: {:?}", e);
            Err((StatusCode::BAD_REQUEST, format!("{:?}", e)))
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
                        serialize(&value)
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
