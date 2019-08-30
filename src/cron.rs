use chrono::{DateTime, Utc};
use std::sync::mpsc::{sync_channel, Receiver, SendError, SyncSender, TryRecvError};
use std::thread::{self, spawn, JoinHandle};

pub type BoxedTask = Box<dyn Task + Send>;

pub trait Task {
    fn when(&self) -> DateTime<Utc>;
    fn call(&self);
}

impl Task for BoxedTask {
    fn when(&self) -> DateTime<Utc> {
        (**self).when()
    }

    fn call(&self) {
        (**self).call()
    }
}

pub struct CronSink {
    channel: SyncSender<BoxedTask>,
    cron_handle: JoinHandle<()>,
}

impl CronSink {
    fn new(channel: SyncSender<BoxedTask>, cron_handle: JoinHandle<()>) -> CronSink {
        Self {
            channel,
            cron_handle,
        }
    }

    pub fn send(&self, task: BoxedTask) -> Result<(), SendError<BoxedTask>> {
        self.channel.send(task)?;
        self.cron_handle.thread().unpark();
        Ok(())
    }
}

pub struct Cron {
    channel: Receiver<BoxedTask>,
    tasks: Vec<BoxedTask>,
}

impl Cron {
    fn new(channel: Receiver<BoxedTask>) -> Self {
        Self {
            channel,
            tasks: vec![],
        }
    }
}

pub fn start() -> CronSink {
    let (sender, receiver) = sync_channel(5);
    CronSink::new(sender, spawn(|| run_cron(Cron::new(receiver))))
}
fn run_cron(mut c: Cron) {
    loop {
        loop {
            match c.channel.try_recv() {
                Ok(task) => c.tasks.push(task),
                Err(TryRecvError::Empty) => break,
                e => {
                    if c.tasks.is_empty() {
                        // If the channel has been disconnected and there are no
                        // more tasks we can terminate this thread.
                        e.unwrap();
                    }
                }
            }
        }
        let mut now = Utc::now();
        c.tasks.drain_filter(|t| {
            if t.when() > now {
                false
            } else {
                t.call();
                now = Utc::now();
                true
            }
        });
        match c.tasks.iter().map(Task::when).min() {
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
