use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{from_reader, to_writer};

use std::fs::File;
use std::sync::mpsc::{sync_channel, Receiver, SendError, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, spawn, JoinHandle};

pub trait Task<U, I: PartialEq = DefaultTaskId> {
    fn when(&self) -> DateTime<Utc>;
    fn call(&self, user_data: &U);
    fn check_id(&self, id: I) -> bool {
        false
    }
}

#[derive(PartialEq)]
struct DefaultTaskId;

pub struct CronSink<T: Task<U, I>, U, I: PartialEq = DefaultTaskId> {
    channel: SyncSender<T>,
    cancel: SyncSender<I>,
    cron_handle: JoinHandle<()>,
}

impl<T: Task<U, I>, U, I: PartialEq> CronSink<T, I, U> {
    fn new(
        channel: SyncSender<T>,
        cancel: SyncSender<I>,
        cron_handle: JoinHandle<()>,
    ) -> Self {
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

    pub fn cancel(&self, task_id: I) -> Result<(), SendError<I>> {
        let boxed = Box::new(task_id);
        self.cancel.send(boxed)?;
        self.cron_handle.thread().unpark();
        Ok(())
    }
}

pub struct Cron<T: Task<U, I>, U, I: PartialEq = DefaultTaskId> {
    channel: Receiver<T>,
    cancel: Receiver<I>,
    user_data: U,
    tasks: Vec<T>,
}

impl<T: Task, I: PartialEq, U> Cron<T, I, U> {
    fn new(user_data: U, channel: Receiver<T>, cancel: Receiver<I>) -> Self {
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
                Ok(id) => self.tasks.retain(|t| !t.id().compare(&*id)),
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
            let http_clone = &self.http;
            self.tasks
                .drain_filter(|t| t.when() < Utc::now())
                .for_each(|t| {
                    let http = Arc::clone(&http_clone);
                    spawn(move || t.call(&http));
                });
        }
        #[cfg(not(feature = "nightly"))]
        {
            let mut i = 0;
            while i < self.tasks.len() {
                if self.tasks[i].when() < Utc::now() {
                    let t = self.tasks.remove(i);
                    let http = Arc::clone(&self.http);
                    spawn(move || t.call(&http));
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

pub fn start<T: Task, I: PartialEq, U>(user_data: U) -> CronSink<T, I, U> {
    let (sender, receiver) = sync_channel(5);
    let (sender_cancel, receiver_cancel) = sync_channel(5);
    CronSink::new(
        sender,
        sender_cancel,
        spawn(|| Cron::new(user_data, receiver, receiver_cancel).run()),
    )
}
