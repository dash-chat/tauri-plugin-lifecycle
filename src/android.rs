use super::LifecycleHandler;
use std::pin::Pin;
use std::sync::OnceLock;

use tauri::{
    plugin::{PluginApi, PluginHandle},
    Manager, Runtime,
};

type ErasedHandler =
    Box<dyn Fn() -> Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;
static PAUSE: OnceLock<ErasedHandler> = OnceLock::new();
static RESUME: OnceLock<ErasedHandler> = OnceLock::new();

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
    let handle =
        api.register_android_plugin("org.dashchat.tauripluginlifecycle", "LifecyclePlugin")?;
    app.manage(LifecycleState(handle));
    Ok(())
}

/// Called from Kotlin's `onStop` (activity fully hidden, analogous to iOS didEnterBackground).
#[no_mangle]
pub extern "C" fn Java_org_dashchat_tauripluginlifecycle_LifecyclePlugin_nativeOnStop(
    _env: jni::JNIEnv,
    _this: jni::objects::JObject,
) {
    if let Some(handler) = PAUSE.get() {
        crate::transitions::push(handler());
    }
}

/// Called from Kotlin's `onStart` (activity becoming visible, analogous to iOS willEnterForeground).
#[no_mangle]
pub extern "C" fn Java_org_dashchat_tauripluginlifecycle_LifecyclePlugin_nativeOnStart(
    _env: jni::JNIEnv,
    _this: jni::objects::JObject,
) {
    if let Some(handler) = RESUME.get() {
        crate::transitions::push(handler());
    }
}
