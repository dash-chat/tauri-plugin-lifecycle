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
type ErasedHandler =
    Box<dyn Fn() -> Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;
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
