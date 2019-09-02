use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{from_reader, to_writer};

use std::fs::File;
use std::sync::mpsc::{sync_channel, Receiver, SendError, SyncSender, TryRecvError};
use std::thread::{self, spawn, JoinHandle};

pub trait Task {
    type Id: Send;
    type UserData: Clone + Send;
    fn when(&self) -> DateTime<Utc>;
    fn call(&self, user_data: Self::UserData);
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
    user_data: T::UserData,
    tasks: Vec<T>,
}

impl<T> Cron<T>
where
    T: Task + Send + Serialize + DeserializeOwned + 'static,
{
    fn new(user_data: T::UserData, channel: Receiver<T>, cancel: Receiver<T::Id>) -> Self {
        let tasks = File::open("files/cron.json")
            .map_err(|e| eprintln!("{:?}", e))
            .and_then(|f| from_reader(f).map_err(|e| eprintln!("{:?}", e)))
            .unwrap_or_else(|_| Vec::new());
        Self {
            channel,
            cancel,
            user_data,
            tasks,
        }
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
                        eprintln!("{:?}", e);
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
                        eprintln!("{:?}", e);
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
                    spawn(move || t.call(http));
                } else {
                    i += 1;
                }
            }
        }
    }

    fn serialize(&self) {
        File::create("files/cron.json")
            .map_err(|e| eprintln!("{:?}", e))
            .and_then(|d| to_writer(d, &self.tasks).map_err(|e| eprintln!("{:?}", e)))
            .unwrap();
    }

    fn run(mut self) {
        loop {
            eprintln!("I'm awake: {:?}", thread::current().id());
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

pub fn start<T>(user_data: T::UserData) -> CronSink<T>
where
    T: Task + Serialize + DeserializeOwned + Send + 'static,
{
    let (sender, receiver) = sync_channel(5);
    let (sender_cancel, receiver_cancel) = sync_channel(5);
    CronSink::new(
        sender,
        sender_cancel,
        spawn(move || Cron::new(user_data, receiver, receiver_cancel).run()),
    )
}
