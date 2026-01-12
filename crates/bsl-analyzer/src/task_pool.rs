//! Task pool for running background tasks with result reporting.
//!
//! This module provides a simple task pool that runs tasks in separate threads
//! and sends results back through a channel.

use crossbeam_channel::{unbounded, Receiver, Sender};

/// A task pool that can spawn tasks and return results through a channel.
pub struct TaskPool<T> {
    /// Sender for task results.
    pub sender: Sender<T>,
}

/// Handle containing both the task pool and result receiver.
pub struct Handle<T> {
    pub pool: TaskPool<T>,
    pub receiver: Receiver<T>,
}

impl<T: Send + 'static> TaskPool<T> {
    /// Creates a new task pool with an associated result receiver.
    pub fn new_with_handle() -> Handle<T> {
        let (sender, receiver) = unbounded();
        Handle { pool: TaskPool { sender }, receiver }
    }

    /// Spawns a task that computes a single result.
    ///
    /// The task runs in a new thread and sends its result back through the channel.
    pub fn spawn<F>(&self, task: F)
    where
        F: FnOnce() -> T + Send + 'static,
    {
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = task();
            let _ = sender.send(result);
        });
    }

    /// Spawns a task that can send multiple results.
    ///
    /// The task receives a sender that it can use to send multiple progress updates
    /// or results back to the main thread.
    pub fn spawn_with_sender<F>(&self, task: F)
    where
        F: FnOnce(Sender<T>) + Send + 'static,
    {
        let sender = self.sender.clone();
        std::thread::spawn(move || task(sender));
    }
}

impl<T: Send + 'static> Default for TaskPool<T> {
    fn default() -> Self {
        Self::new_with_handle().pool
    }
}
