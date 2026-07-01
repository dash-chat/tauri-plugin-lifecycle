import Tauri
import UIKit
import WebKit

// Implemented in Rust (src/lib.rs), linked with C ABI.
@_silgen_name("tauri_plugin_ios_lifecycle_did_enter_background")
func rustDidEnterBackground(
  _ done: @convention(c) (UnsafeMutableRawPointer?) -> Void,
  _ ctx: UnsafeMutableRawPointer?
)

@_silgen_name("tauri_plugin_ios_lifecycle_will_enter_foreground")
func rustWillEnterForeground()

// Boxes the "end the background-task assertion" closure so it can be handed to
// Rust as an opaque context and invoked once the async pause work completes.
private final class EndTaskBox {
  let end: () -> Void
  init(_ end: @escaping () -> Void) { self.end = end }
}

// C trampoline Rust calls with the boxed context when the pause future resolves.
private let endTaskTrampoline: @convention(c) (UnsafeMutableRawPointer?) -> Void = { ctx in
  guard let ctx = ctx else { return }
  Unmanaged<EndTaskBox>.fromOpaque(ctx).takeRetainedValue().end()
}

class LifecyclePlugin: Plugin {
  private var observers: [NSObjectProtocol] = []

  override func load(webview: WKWebView) {
    let center = NotificationCenter.default

    observers.append(
      center.addObserver(
        forName: UIApplication.didEnterBackgroundNotification,
        object: nil,
        queue: .main
      ) { _ in
        let app = UIApplication.shared
        var bgTask: UIBackgroundTaskIdentifier = .invalid
        bgTask = app.beginBackgroundTask(withName: "tauri-plugin-ios-lifecycle") {
          if bgTask != .invalid {
            app.endBackgroundTask(bgTask)
            bgTask = .invalid
          }
        }
        let box = EndTaskBox {
          DispatchQueue.main.async {
            if bgTask != .invalid {
              app.endBackgroundTask(bgTask)
              bgTask = .invalid
            }
          }
        }
        rustDidEnterBackground(endTaskTrampoline, Unmanaged.passRetained(box).toOpaque())
      }
    )

    observers.append(
      center.addObserver(
        forName: UIApplication.willEnterForegroundNotification,
        object: nil,
        queue: .main
      ) { _ in
        rustWillEnterForeground()
      }
    )
  }

  deinit {
    for observer in observers {
      NotificationCenter.default.removeObserver(observer)
    }
  }
}

@_cdecl("init_plugin_ios_lifecycle")
func initPlugin() -> Plugin {
  return LifecyclePlugin()
}
