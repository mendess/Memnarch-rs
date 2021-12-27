use crate::util::{Mutex, MutexGuard};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    error::Error,
    fmt::Debug,
    io::{self, Write},
    ops::{Deref, DerefMut},
    path::PathBuf,
};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    // sync::{Mutex, MutexGuard},
};

pub struct Database<T, E = io::Error> {
    filename: Mutex<PathBuf>,
    serializer: Box<dyn Fn(&mut dyn Write, &T) -> Result<(), E> + Sync + Send>,
    deserializer: Box<dyn Fn(&[u8]) -> Result<T, E> + Sync + Send>,
}

impl<T: DeserializeOwned + Serialize> Database<T, io::Error> {
    pub fn new<P: Into<PathBuf>>(filename: P) -> Self {
        Self::with_ser_and_deser(
            filename,
            |w, t| serde_json::to_writer(w, t).map_err(Into::into),
            |s| serde_json::from_slice(s).map_err(Into::into),
        )
    }
}

impl<T, E: Into<Box<dyn Error>>> Database<T, E> {
    pub fn with_ser_and_deser<P, S, D>(filename: P, serializer: S, deserializer: D) -> Self
    where
        P: Into<PathBuf>,
        S: Fn(&mut dyn Write, &T) -> Result<(), E> + Sync + Send + 'static,
        D: Fn(&[u8]) -> Result<T, E> + Sync + Send + 'static,
    {
        let filename = filename.into();
        let filename_display = Box::leak(filename.display().to_string().into_boxed_str());
        Self {
            filename: Mutex::new(filename, filename_display, 0),
            serializer: Box::new(move |w, t| serializer(w, t)),
            deserializer: Box::new(move |slice| deserializer(slice)),
        }
    }
}

impl<T, E> Database<T, E>
where
    T: Default + Debug,
    E: Into<anyhow::Error>,
    E: From<io::Error>,
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

pub struct DbGuard<'db, T: Debug, E: Into<anyhow::Error> = serde_json::Error> {
    pathbuf: MutexGuard<'db, PathBuf>,
    serializer: &'db (dyn Fn(&mut dyn Write, &T) -> Result<(), E> + Send + Sync),
    t: T,
    save: bool,
}

impl<'db, T: Debug, E: Into<anyhow::Error>> Deref for DbGuard<'db, T, E> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.t
    }
}

impl<'db, T: Debug, E: Into<anyhow::Error>> DerefMut for DbGuard<'db, T, E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.t
    }
}

impl<'db, T: Default + Debug, E: Into<anyhow::Error>> DbGuard<'db, T, E> {
    pub fn take(&mut self) -> T {
        self.save = false;
        std::mem::take(&mut self.t)
    }
}

impl<'db, T: Debug, E: Into<anyhow::Error>> Drop for DbGuard<'db, T, E> {
    fn drop(&mut self) {
        if self.save {
            let (mut temp_file, temp_path) =
                match tempfile::NamedTempFile::new_in(".").map(|f| f.into_parts()) {
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
                    e.into()
                );
            }
            if let Err(e) = std::fs::rename(&temp_path, &*self.pathbuf) {
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
