//! React to iOS app background/foreground transitions from Rust.
//!
//! iOS SIGKILLs a suspended app that still holds a file lock — an open SQLite
//! connection pool being the usual culprit (`0xdead10cc`). Releasing such locks
//! must happen *before* suspension, which needs a `UIBackgroundTask` assertion
//! to keep the app alive just long enough for an async teardown to finish.
//!
//! This plugin observes `didEnterBackground` / `willEnterForeground` natively
//! (not tao's `willResignActive`, which also fires on transient interruptions
//! like Control Center), holds the assertion while your `on_pause` future runs,
//! and rebuilds via `on_resume` on foreground.
//!
//! ```no_run
//! tauri::Builder::default().plugin(
//!     tauri_plugin_lifecycle::Builder::new()
//!         .on_pause(|app| async move { /* release resources */ })
//!         .on_resume(|app| async move { /* re-acquire resources */ })
//!         .build(),
//! );
//! ```

use std::future::Future;
use std::pin::Pin;

use tauri::{
    plugin::{Builder as PluginBuilder, TauriPlugin},
    AppHandle, Runtime,
};

type LifecycleHandler<R> =
    Box<dyn Fn(AppHandle<R>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static>;

/// Builder for the iOS lifecycle plugin. Register async handlers that run when
/// the app enters the background (`on_pause`) and returns to the foreground
/// (`on_resume`). On non-iOS platforms the handlers are never invoked.
#[derive(Default)]
pub struct Builder<R: Runtime> {
    on_pause: Option<LifecycleHandler<R>>,
    on_resume: Option<LifecycleHandler<R>>,
}

impl<R: Runtime> Builder<R> {
    pub fn new() -> Self {
        Self {
            on_pause: None,
            on_resume: None,
        }
    }

    /// Runs when the app enters the background. The app stays alive under a
    /// `UIBackgroundTask` assertion until this future resolves, so use it to
    /// flush/close anything holding a file lock. Keep it fast (≈ seconds).
    pub fn on_pause<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(AppHandle<R>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.on_pause = Some(Box::new(move |app| Box::pin(handler(app))));
        self
    }

    /// Runs when the app returns to the foreground.
    pub fn on_resume<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(AppHandle<R>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.on_resume = Some(Box::new(move |app| Box::pin(handler(app))));
        self
    }

    pub fn build(self) -> TauriPlugin<R> {
        let on_pause = self.on_pause;
        let on_resume = self.on_resume;
        PluginBuilder::new("lifecycle")
            .setup(move |_app, _api| {
                #[cfg(target_os = "ios")]
                ios::setup(_app, _api, on_pause, on_resume)?;
                #[cfg(target_os = "android")]
                android::setup(_app, _api, on_pause, on_resume)?;
                #[cfg(not(any(target_os = "ios", target_os = "android")))]
                {
                    // Handlers only ever fire on iOS/Android; drop them elsewhere.
                    let _ = (&on_pause, &on_resume);
                }
                Ok(())
            })
            .build()
    }
}

#[cfg(target_os = "ios")]
mod ios;

#[cfg(target_os = "android")]
mod android;
