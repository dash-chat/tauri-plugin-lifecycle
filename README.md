# tauri-plugin-lifecycle

React to mobile app **background / foreground** transitions from Rust. Currently supports only iOS and Android.

— on iOS, uses a
`UIBackgroundTask` assertion so an async teardown can finish before iOS suspends
the app.
- on Android, uses an onStop handler giving time to start foreground tasks if needed.

## Why

### iOS

iOS SIGKILLs a suspended app that still holds a file lock (an open SQLite
connection pool is the classic case — termination reason `RUNNINGBOARD
0xdead10cc`). Releasing those locks must happen *before* suspension. That needs:

1. the **true-background** signal (`applicationDidEnterBackground`) — not
   `applicationWillResignActive`, which also fires on transient interruptions
   (Control Center, incoming calls); and
2. a **`beginBackgroundTask` assertion** to keep the process alive just long
   enough for the async cleanup to complete.

Tauri core's `WindowEvent::Suspended` maps to `willResignActive` (wrong signal),
and no existing plugin holds the background-task assertion. This plugin does both.

### Android

If an app wants to start an android "Foreground Task", it only gets a small time window to do so when switching to another app. The `tauri-plugin-background-service` crate, for example, typically launches these from the front end.

While checking from the front-end that the webview has been unloaded usually works, this might be initiated slightly later than the app has received a lifecycle notification of the switch, meaning the time budget to start a task has been exhausted.

## Usage

```rust
tauri::Builder::default().plugin(
    tauri_plugin_lifecycle::Builder::new()
        .on_pause(|app| async move {
            // release everything holding a file lock (close DB pools, stop tasks)
        })
        .on_resume(|app| async move {
            // re-acquire on foreground
        })
        .build(),
);
```

`on_pause` runs under the background-task assertion; the app stays alive until
its future resolves, then the assertion is released and iOS suspends cleanly.
Keep it fast (seconds).

## How it works

### iOS

The Swift side (`ios/Sources`) observes `didEnterBackgroundNotification` and
`willEnterForegroundNotification` via `NSNotificationCenter` (coexisting with
tao's `AppDelegate`), begins the assertion, and calls into Rust over a direct C
ABI. The Rust side runs your handler on the Tauri async runtime and signals
completion back so Swift ends the assertion. No webview round-trip — it works
while the webview is suspended.

### Android

The `on_pause` handler in this plugin is launched by overriding the `onStop` method in an Android lifecycle plugin, as this represents the moment of switching to another app or ending this app.

The `on_resume` handler in this plugin uses the `onStart` android method.