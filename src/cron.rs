use crate::consts::FILES_DIR;
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{from_reader, to_writer};
use std::{
    error::Error,
    fs::{DirBuilder, File},
    path::{Path, PathBuf},
    sync::mpsc::{sync_channel, Receiver, SendError, SyncSender, TryRecvError},
    thread::{self, spawn, JoinHandle},
};

const CRON_DIR: &str = "cron";

pub trait Task {
    type Id: Send;
    type GlobalData: Clone + Send;
    fn when(&self) -> DateTime<Utc>;
    fn call(&self, user_data: Self::GlobalData) -> Result<(), Box<dyn Error>>;
    fn check_id(&self, _: &Self::Id) -> bool {
        false
    }
}

pub struct CronSink<T: Task> {
    channel: SyncSender<T>,
    cancel: SyncSender<T::Id>,
    cron_handle: JoinHandle<()>,
}

impl<T: Task> CronSink<T> {
    fn new(channel: SyncSender<T>, cancel: SyncSender<T::Id>, cron_handle: JoinHandle<()>) -> Self {
        Self {
            channel,
            cancel,
            cron_handle,
        }
    }

    pub fn send(&self, task: T) -> Result<(), SendError<T>> {
        self.channel.send(task)?;
        self.cron_handle.thread().unpark();
        Ok(())
    }

    pub fn cancel(&self, task_id: T::Id) -> Result<(), SendError<T::Id>> {
        self.cancel.send(task_id)?;
        self.cron_handle.thread().unpark();
        Ok(())
    }
}

pub struct Cron<T: Task> {
    channel: Receiver<T>,
    cancel: Receiver<T::Id>,
    user_data: T::GlobalData,
    tasks: Vec<T>,
    path: Box<Path>,
}

impl<T> Cron<T>
where
    T: Task + Send + Serialize + DeserializeOwned + 'static,
{
    fn new(
        path: &str,
        user_data: T::GlobalData,
        channel: Receiver<T>,
        cancel: Receiver<T::Id>,
    ) -> std::io::Result<Self> {
        let path = [FILES_DIR, CRON_DIR, path].iter().collect::<PathBuf>();
        DirBuilder::new()
            .recursive(true)
            .create(path.parent().unwrap())?;
        let tasks = File::open(&path)
            .map_err(|e| eprintln!("Failed to open tasks file for {}: {:?}", path.display(), e))
            .and_then(|f| from_reader(f).map_err(|e| eprintln!("Error parsing cron.json: {}", e)))
            .unwrap_or_else(|_| Vec::new());
        Ok(Self {
            channel,
            cancel,
            user_data,
            tasks,
            path: path.into_boxed_path(),
        })
    }

    fn receive(&mut self) -> TryRecvError {
        loop {
            match self.channel.try_recv() {
                Ok(task) => self.tasks.push(task),
                Err(TryRecvError::Empty) => break TryRecvError::Empty,
                Err(e) => {
                    if self.tasks.is_empty() {
                        // If the channel has been disconnected and there are no
                        // more tasks we can terminate this thread.
                        eprintln!("Channel disconnected for {}: {:?}", self.path.display(), e);
                        break TryRecvError::Disconnected;
                    }
                }
            }
        }
    }

    fn cancel(&mut self) -> TryRecvError {
        loop {
            match self.cancel.try_recv() {
                Ok(id) => self.tasks.retain(|t| !t.check_id(&id)),
                Err(TryRecvError::Empty) => break TryRecvError::Empty,
                Err(e) => {
                    if self.tasks.is_empty() {
                        eprintln!(
                            "Cancel channel disconneted for {}: {:?}",
                            self.path.display(),
                            e
                        );
                        break TryRecvError::Disconnected;
                    }
                }
            }
        }
    }

    fn run_tasks(&mut self) {
        #[cfg(feature = "nightly")]
        {
            let user_data = &self.user_data;
            self.tasks
                .drain_filter(|t| t.when() < Utc::now())
                .for_each(|t| {
                    let clone = Clone::clone(&user_data);
                    spawn(move || t.call(clone));
                });
        }
        #[cfg(not(feature = "nightly"))]
        {
            let mut i = 0;
            while i < self.tasks.len() {
                if self.tasks[i].when() < Utc::now() {
                    let t = self.tasks.remove(i);
                    let http = Clone::clone(&self.user_data);
                    spawn(move || {
                        let _ = t.call(http).map_err(|e| eprintln!("{}", e));
                    });
                } else {
                    i += 1;
                }
            }
        }
    }

    fn serialize(&self) {
        DirBuilder::new()
            .recursive(true)
            .create(self.path.parent().unwrap())
            .and_then(|_| File::create(&self.path))
            .and_then(|d| to_writer(d, &self.tasks).map_err(|e| e.into()))
            .map_err(|e| eprintln!("{}", e))
            .ok();
    }

    fn run(mut self) {
        loop {
            eprintln!("I'm awake: {:?}: {:?}", thread::current().id(), self.path);
            if TryRecvError::Disconnected == self.receive() && self.tasks.is_empty() {
                return;
            }
            if TryRecvError::Disconnected == self.cancel() {
                return;
            }
            self.run_tasks();
            self.serialize();
            match self.tasks.iter().map(|t| t.when()).min() {
                None => thread::park(),
                Some(smallest_timeout) => {
                    match smallest_timeout.signed_duration_since(Utc::now()).to_std() {
                        Ok(timeout) => thread::park_timeout(timeout),
                        Err(e) => eprintln!("Sheduling to the past? {:?}", e),
                    }
                }
            }
        }
    }
}

pub fn start<T>(path: &str, user_data: T::GlobalData) -> CronSink<T>
where
    T: Task + Serialize + DeserializeOwned + Send + 'static,
{
    let (sender, receiver) = sync_channel(5);
    let (sender_cancel, receiver_cancel) = sync_channel(5);
    let cron = Cron::new(path, user_data, receiver, receiver_cancel);
    CronSink::new(
        sender,
        sender_cancel,
        spawn(move || match cron {
            Ok(cron) => cron.run(),
            Err(e) => eprintln!("Warning: cron is not running: {:?}", e),
        }),
    )
}
