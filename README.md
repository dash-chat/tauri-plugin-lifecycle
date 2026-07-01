# tauri-plugin-ios-lifecycle

React to iOS app **background / foreground** transitions from Rust — with a
`UIBackgroundTask` assertion so an async teardown can finish before iOS suspends
the app.

## Why

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

## Usage

```rust
tauri::Builder::default().plugin(
    tauri_plugin_ios_lifecycle::Builder::new()
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
Keep it fast (seconds). On non-iOS platforms the handlers are never invoked.

## How it works

The Swift side (`ios/Sources`) observes `didEnterBackgroundNotification` and
`willEnterForegroundNotification` via `NSNotificationCenter` (coexisting with
tao's `AppDelegate`), begins the assertion, and calls into Rust over a direct C
ABI. The Rust side runs your handler on the Tauri async runtime and signals
completion back so Swift ends the assertion. No webview round-trip — it works
while the webview is suspended.
