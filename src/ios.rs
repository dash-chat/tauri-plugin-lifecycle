use super::LifecycleHandler;
use std::ffi::c_void;
use std::pin::Pin;
use std::sync::OnceLock;

use tauri::{
    plugin::{PluginApi, PluginHandle},
    Manager, Runtime,
};

tauri::ios_plugin_binding!(init_plugin_lifecycle);

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
    crate::transitions::start();
    let handle = api.register_ios_plugin(init_plugin_lifecycle)?;
    app.manage(LifecycleState(handle));
    Ok(())
}

/// Raw pointer wrapper so the Swift-owned completion context can cross the
/// spawn boundary. Safe: it is only the opaque `ctx` we hand straight back to
/// `done`, never dereferenced in Rust.
struct SendPtr(*mut c_void);
unsafe impl Send for SendPtr {}

/// Ends Swift's background-task assertion when dropped: once the pause future
/// resolves, or when a resume drops it first.
struct EndBackgroundTask {
    done: extern "C" fn(*mut c_void),
    ctx: SendPtr,
}

impl Drop for EndBackgroundTask {
    fn drop(&mut self) {
        (self.done)(self.ctx.0);
    }
}

/// Called from Swift's `didEnterBackground` observer (`ios/Sources`). Queues
/// the registered pause future, and ends Swift's background-task assertion
/// once it resolves or is dropped. If no handler is registered, ends it
/// immediately.
///
/// # Safety
/// `done` must be a valid C function pointer and `ctx` the context it expects.
#[no_mangle]
pub extern "C" fn tauri_plugin_lifecycle_did_enter_background(
    done: extern "C" fn(*mut c_void),
    ctx: *mut c_void,
) {
    let end_background_task = EndBackgroundTask {
        done,
        ctx: SendPtr(ctx),
    };
    if let Some(handler) = PAUSE.get() {
        let pause = handler();
        crate::transitions::push(Box::pin(async move {
            let _end_background_task = end_background_task;
            pause.await;
        }));
    }
}

/// Called from Swift's `willEnterForeground` observer.
#[no_mangle]
pub extern "C" fn tauri_plugin_lifecycle_will_enter_foreground() {
    if let Some(handler) = RESUME.get() {
        crate::transitions::push(handler());
    }
}
