use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{from_reader, to_writer};
use serenity::http::raw::Http;

use std::fs::File;
use std::sync::mpsc::{sync_channel, Receiver, SendError, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, spawn, JoinHandle};

pub trait Task {
    fn when(&self) -> DateTime<Utc>;
    fn call(&self, http: &Http);
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TaskKind {
    Remindme(crate::commands::general::Reminder),
}

impl From<crate::commands::general::Reminder> for TaskKind {
    fn from(r: crate::commands::general::Reminder) -> Self {
        TaskKind::Remindme(r)
    }
}

impl TaskKind {
    fn as_task(&self) -> &dyn Task {
        match self {
            TaskKind::Remindme(r) => r,
        }
    }

    fn when(&self) -> DateTime<Utc> {
        self.as_task().when()
    }

    fn call(&self, http: &Http) {
        self.as_task().call(http)
    }
}

pub struct CronSink {
    channel: SyncSender<TaskKind>,
    cron_handle: JoinHandle<()>,
}

impl CronSink {
    fn new(channel: SyncSender<TaskKind>, cron_handle: JoinHandle<()>) -> CronSink {
        Self {
            channel,
            cron_handle,
        }
    }

    pub fn send(&self, task: TaskKind) -> Result<(), SendError<TaskKind>> {
        self.channel.send(task)?;
        self.cron_handle.thread().unpark();
        Ok(())
    }
}

pub struct Cron {
    channel: Receiver<TaskKind>,
    http: Arc<Http>,
    tasks: Vec<TaskKind>,
}

impl Cron {
    fn new(http: Arc<Http>, channel: Receiver<TaskKind>) -> Self {
        let tasks = File::open("files/cron.json")
            .map_err(|e| eprintln!("{:?}", e))
            .and_then(|f| from_reader(f).map_err(|e| eprintln!("{:?}", e)))
            .unwrap_or_else(|_| Vec::new());
        Self {
            channel,
            http,
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
}

pub fn start(http: Arc<Http>) -> CronSink {
    let (sender, receiver) = sync_channel(5);
    CronSink::new(sender, spawn(|| run_cron(Cron::new(http, receiver))))
}

fn run_cron(mut c: Cron) {
    loop {
        if TryRecvError::Disconnected == c.receive() && c.tasks.is_empty() {
            return;
        }
        c.run_tasks();
        c.serialize();
        match c.tasks.iter().map(|t| t.when()).min() {
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
