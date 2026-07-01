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
//!     tauri_plugin_ios_lifecycle::Builder::new()
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
        PluginBuilder::new("ios-lifecycle")
            .setup(move |_app, _api| {
                #[cfg(target_os = "ios")]
                imp::setup(_app, _api, on_pause, on_resume)?;
                #[cfg(not(target_os = "ios"))]
                {
                    // Handlers only ever fire on iOS; drop them elsewhere.
                    let _ = (&on_pause, &on_resume);
                }
                Ok(())
            })
            .build()
    }
}

#[cfg(target_os = "ios")]
mod imp {
    use super::LifecycleHandler;
    use std::ffi::c_void;
    use std::pin::Pin;
    use std::sync::OnceLock;

    use tauri::{
        plugin::{PluginApi, PluginHandle},
        Manager, Runtime,
    };

    tauri::ios_plugin_binding!(init_plugin_ios_lifecycle);

    // Type-erased handlers: the concrete `AppHandle<R>` is captured inside, so no
    // generic parameter leaks into these globals. Set once at plugin setup.
    type ErasedHandler = Box<dyn Fn() -> Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;
    static PAUSE: OnceLock<ErasedHandler> = OnceLock::new();
    static RESUME: OnceLock<ErasedHandler> = OnceLock::new();

    // Keeps the Swift `Plugin` instance (and its lifecycle observers) alive for
    // the app's lifetime.
    struct LifecycleState<R: Runtime>(#[allow(dead_code)] PluginHandle<R>);

    pub fn setup<R: Runtime, C: serde::de::DeserializeOwned>(
        app: &tauri::AppHandle<R>,
        api: PluginApi<R, C>,
        on_pause: Option<LifecycleHandler<R>>,
        on_resume: Option<LifecycleHandler<R>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(handler) = on_pause {
            let app = app.clone();
            let _ = PAUSE.set(Box::new(move || handler(app.clone())));
        }
        if let Some(handler) = on_resume {
            let app = app.clone();
            let _ = RESUME.set(Box::new(move || handler(app.clone())));
        }
        let handle = api.register_ios_plugin(init_plugin_ios_lifecycle)?;
        app.manage(LifecycleState(handle));
        Ok(())
    }

    /// Raw pointer wrapper so the Swift-owned completion context can cross the
    /// spawn boundary. Safe: it is only the opaque `ctx` we hand straight back to
    /// `done`, never dereferenced in Rust.
    struct SendPtr(*mut c_void);
    unsafe impl Send for SendPtr {}

    /// Called from Swift's `didEnterBackground` observer (`ios/Sources`). Runs the
    /// registered pause future, then invokes `done(ctx)` so Swift can end its
    /// background-task assertion. If no handler is registered, ends it immediately.
    ///
    /// # Safety
    /// `done` must be a valid C function pointer and `ctx` the context it expects.
    #[no_mangle]
    pub extern "C" fn tauri_plugin_ios_lifecycle_did_enter_background(
        done: extern "C" fn(*mut c_void),
        ctx: *mut c_void,
    ) {
        let ctx = SendPtr(ctx);
        match PAUSE.get() {
            Some(handler) => {
                let fut = handler();
                tauri::async_runtime::spawn(async move {
                    fut.await;
                    let ctx = ctx;
                    done(ctx.0);
                });
            }
            None => done(ctx.0),
        }
    }

    /// Called from Swift's `willEnterForeground` observer.
    #[no_mangle]
    pub extern "C" fn tauri_plugin_ios_lifecycle_will_enter_foreground() {
        if let Some(handler) = RESUME.get() {
            let fut = handler();
            tauri::async_runtime::spawn(fut);
        }
    }
}
