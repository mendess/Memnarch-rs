pub mod json_hash_map;

use serde::{de::DeserializeOwned, Serialize};
use std::{
    fmt::Debug,
    io::{self, Write},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    sync::{Mutex, MutexGuard, OnceCell},
};

type Serializer<T, E> = Box<dyn Fn(&mut dyn Write, &T) -> Result<(), E> + Sync + Send>;

type Deserializer<T, E> = Box<dyn Fn(&[u8]) -> Result<T, E> + Sync + Send>;

pub struct Database<T, E: std::error::Error = io::Error> {
    filename: Mutex<PathBuf>,
    serializer: Serializer<T, E>,
    deserializer: Deserializer<T, E>,
}

pub struct GlobalDatabase<T, E: std::error::Error = io::Error> {
    db: OnceCell<Database<T, E>>,
    filename: &'static str,
}

impl<T> GlobalDatabase<T, io::Error>
where
    T: Serialize + DeserializeOwned + Default + Debug,
{
    pub async fn load(&self) -> io::Result<DbGuard<'_, T, io::Error>> {
        self.db
            .get_or_try_init(|| async { Database::new(self.filename).await })
            .await?
            .load()
            .await
    }
}

impl<T: DeserializeOwned + Serialize> Database<T, io::Error> {
    pub async fn new<P: Into<PathBuf>>(filename: P) -> io::Result<Self> {
        Self::with_ser_and_deser(
            filename,
            |w, t| serde_json::to_writer(w, t).map_err(Into::into),
            |s| serde_json::from_slice(s).map_err(Into::into),
        )
        .await
    }

    pub const fn const_new(filename: &'static str) -> GlobalDatabase<T> {
        GlobalDatabase {
            db: OnceCell::const_new(),
            filename,
        }
    }
}

impl<T, E: std::error::Error> Database<T, E> {
    pub async fn with_ser_and_deser<P, S, D>(
        filename: P,
        serializer: S,
        deserializer: D,
    ) -> io::Result<Self>
    where
        P: Into<PathBuf>,
        S: Fn(&mut dyn Write, &T) -> Result<(), E> + Sync + Send + 'static,
        D: Fn(&[u8]) -> Result<T, E> + Sync + Send + 'static,
    {
        let filename: PathBuf = filename.into();
        let dir = filename
            .parent()
            .expect("databases have to point to normal files");
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            filename: Mutex::new(filename),
            serializer: Box::new(move |w, t| serializer(w, t)),
            deserializer: Box::new(move |slice| deserializer(slice)),
        })
    }
}

impl<T, E> Database<T, E>
where
    T: Default + Debug,
    E: From<io::Error> + std::error::Error,
{
    pub async fn load(&self) -> Result<DbGuard<'_, T, E>, E> {
        let pathbuf = self.filename.lock().await;
        let mut file = match File::open(&*pathbuf).await {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(DbGuard {
                    pathbuf,
                    serializer: &*self.serializer,
                    t: Default::default(),
                    save: true,
                });
            }
            Err(e) => return Err(e.into()),
        };
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await?;
        let t = (self.deserializer)(&buf)?;
        Ok(DbGuard {
            pathbuf,
            serializer: &*self.serializer,
            t,
            save: true,
        })
    }
}

pub struct DbGuard<'db, T: Debug, E: std::error::Error> {
    pathbuf: MutexGuard<'db, PathBuf>,
    serializer: &'db (dyn Fn(&mut dyn Write, &T) -> Result<(), E> + Send + Sync),
    t: T,
    save: bool,
}

impl<'db, T: Debug, E: std::error::Error> Deref for DbGuard<'db, T, E> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.t
    }
}

impl<'db, T: Debug, E: std::error::Error> DerefMut for DbGuard<'db, T, E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.t
    }
}

impl<'db, T: Default + Debug, E: std::error::Error> DbGuard<'db, T, E> {
    pub fn take(&mut self) -> T {
        self.save = false;
        std::mem::take(&mut self.t)
    }
}

impl<'db, T: Debug, E: std::error::Error> Drop for DbGuard<'db, T, E> {
    fn drop(&mut self) {
        if self.save {
            let dirname = self.pathbuf.parent().unwrap_or_else(|| Path::new("/"));
            let (mut temp_file, temp_path) =
                match tempfile::NamedTempFile::new_in(dirname).map(|f| f.into_parts()) {
                    Ok(f) => f,
                    Err(e) => {
                        log::error!(
                            "failed to create temporary file for '{}': {}",
                            self.pathbuf.display(),
                            e
                        );
                        return;
                    }
                };
            if let Err(e) = (self.serializer)(&mut temp_file, &self.t) {
                log::error!(
                    "Failed to store to tempfile for '{}': {:?}",
                    self.pathbuf.display(),
                    e,
                );
            }
            if let Err(e) = std::fs::rename(&temp_path, &**self.pathbuf) {
                log::error!(
                    "Failed to rename '{}' to '{}': {}",
                    temp_path.display(),
                    self.pathbuf.display(),
                    e
                );
            }
        }
    }
}
