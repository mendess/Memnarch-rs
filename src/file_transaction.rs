use serde::{de::DeserializeOwned, Serialize};
use std::{
    fmt::Debug,
    io,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    path::PathBuf,
};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    sync::{Mutex, MutexGuard},
};

pub struct Database<T> {
    filename: Mutex<PathBuf>,
    _marker: PhantomData<T>,
}

impl<T> Database<T> {
    pub fn new<P: Into<PathBuf>>(filename: P) -> Self {
        Self {
            filename: Mutex::new(filename.into()),
            _marker: PhantomData,
        }
    }
}

impl<T: DeserializeOwned + Serialize + Default + Debug> Database<T> {
    pub async fn load(&self) -> io::Result<DbGuard<'_, T>> {
        let pathbuf = self.filename.lock().await;
        let mut file = match File::open(&*pathbuf).await {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(DbGuard {
                    pathbuf,
                    t: Default::default(),
                    save: true,
                });
            }
            Err(e) => return Err(e),
        };
        let mut buf = String::new();
        file.read_to_string(&mut buf).await?;
        let t = serde_json::from_str::<T>(&buf)?;
        Ok(DbGuard {
            pathbuf,
            t,
            save: true,
        })
    }
}

pub struct DbGuard<'db, T: Serialize + Debug> {
    pathbuf: MutexGuard<'db, PathBuf>,
    t: T,
    save: bool,
}

impl<'db, T: Serialize + Debug> Deref for DbGuard<'db, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.t
    }
}

impl<'db, T: Serialize + Debug> DerefMut for DbGuard<'db, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.t
    }
}

impl<'db, T: Serialize + Default + Debug> DbGuard<'db, T> {
    pub fn take(&mut self) -> T {
        self.save = false;
        std::mem::take(&mut self.t)
    }
}

impl<'db, T: Serialize + Debug> Drop for DbGuard<'db, T> {
    fn drop(&mut self) {
        if self.save {
            let (temp_file, temp_path) =
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
            if let Err(e) = serde_json::to_writer(temp_file, &self.t) {
                log::error!(
                    "Failed to store to tempfile for '{}': {}",
                    self.pathbuf.display(),
                    e
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
