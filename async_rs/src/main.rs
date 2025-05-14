use std::{
    pin::Pin,
    sync::{Arc, Mutex, mpsc},
    task::{Context, Poll, Waker},
    time::Duration,
};

use futures::task::{self, ArcWake};
use tokio::time::Instant;

fn main() {
    let mut exe = MiniTokio::new();

    exe.spawn(async {
        let time = Duration::from_secs(5);
        let f = Delay::new(time).await;
        println!("{f}");
        println!("finish secs")
    });

    exe.spawn(async {
        let time = Duration::from_millis(1);
        let f = Delay::new(time).await;
        println!("{f}");
        println!("finish millis")
    });
    exe.run();
}

struct MiniTokio {
    scheduled: mpsc::Receiver<Arc<Task>>,
    sender: mpsc::Sender<Arc<Task>>,
}

impl MiniTokio {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();

        MiniTokio {
            scheduled: rx,
            sender: tx,
        }
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Task::spawn(future, &self.sender);
    }

    pub fn run(&mut self) {
        while let Ok(task) = self.scheduled.recv() {
            task.poll();
        }
    }
}

struct TaskFuture {
    future: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    poll: Poll<()>,
}

impl TaskFuture {
    pub fn new(future: impl Future<Output = ()> + Send + 'static) -> Self {
        TaskFuture {
            future: Box::pin(future),
            poll: Poll::Pending,
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) {
        if self.poll.is_pending() {
            self.poll = self.future.as_mut().poll(cx);
        }
    }
}

struct Task {
    tasks: Mutex<TaskFuture>,
    sender: mpsc::Sender<Arc<Task>>,
}

impl Task {
    fn poll(self: Arc<Self>) {
        let waker = task::waker(self.clone());
        let mut cx = Context::from_waker(&waker);

        let mut f = self.tasks.try_lock().unwrap();
        f.poll(&mut cx);
    }

    pub fn spawn<F>(future: F, sender: &mpsc::Sender<Arc<Task>>)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task = TaskFuture::new(future);
        let task = Arc::new(Task {
            tasks: Mutex::new(task),
            sender: sender.clone(),
        });

        let _ = sender.send(task);
    }
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.sender.send(arc_self.clone()).unwrap();
    }
}

struct Delay {
    when: Instant,
    waker: Option<Arc<Mutex<Waker>>>,
}

impl Delay {
    fn new(time: Duration) -> Self {
        Delay {
            when: Instant::now() + time,
            waker: None,
        }
    }
}

impl Future for Delay {
    type Output = String;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() >= self.when {
            return Poll::Ready("Hello Async!".to_string());
        }

        if let Some(waker) = &self.waker {
            let mut waker = waker.lock().unwrap();

            if !waker.will_wake(cx.waker()) {
                *waker = cx.waker().clone();
            }
        } else {
            let waker = Arc::new(Mutex::new(cx.waker().clone()));
            let when = self.when;
            self.waker = Some(waker.clone());

            std::thread::spawn(move || {
                std::thread::sleep(when - Instant::now());
                let lock = waker.try_lock().unwrap();
                lock.wake_by_ref();
            });
        }
        Poll::Pending
    }
}
