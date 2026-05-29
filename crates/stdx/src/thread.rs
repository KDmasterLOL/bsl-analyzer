use std::fmt;

mod intent;
mod pool;

pub use intent::ThreadIntent;
pub use pool::Pool;

pub fn spawn<F, T>(intent: ThreadIntent, name: String, f: F) -> JoinHandle<T>
where
    F: (FnOnce() -> T) + Send + 'static,
    T: Send + 'static,
{
    Builder::new(intent, name).spawn(f).expect("failed to spawn thread")
}

pub struct Builder {
    intent: ThreadIntent,
    inner: jod_thread::Builder,
    allow_leak: bool,
}

impl Builder {
    #[must_use]
    pub fn new(intent: ThreadIntent, name: impl Into<String>) -> Self {
        Self { intent, inner: jod_thread::Builder::new().name(name.into()), allow_leak: false }
    }

    #[must_use]
    pub fn stack_size(self, size: usize) -> Self {
        Self { inner: self.inner.stack_size(size), ..self }
    }

    #[must_use]
    pub fn allow_leak(self, allow_leak: bool) -> Self {
        Self { allow_leak, ..self }
    }

    pub fn spawn<F, T>(self, f: F) -> std::io::Result<JoinHandle<T>>
    where
        F: (FnOnce() -> T) + Send + 'static,
        T: Send + 'static,
    {
        let inner_handle = self.inner.spawn(move || {
            self.intent.apply_to_current_thread();
            f()
        })?;

        Ok(JoinHandle { inner: Some(inner_handle), allow_leak: self.allow_leak })
    }
}

pub struct JoinHandle<T = ()> {
    inner: Option<jod_thread::JoinHandle<T>>,
    allow_leak: bool,
}

impl<T> JoinHandle<T> {
    #[must_use]
    pub fn join(mut self) -> T {
        self.inner.take().unwrap().join()
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        if !self.allow_leak {
            return;
        }

        if let Some(join_handle) = self.inner.take() {
            join_handle.detach();
        }
    }
}

impl<T> fmt::Debug for JoinHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("JoinHandle { .. }")
    }
}
