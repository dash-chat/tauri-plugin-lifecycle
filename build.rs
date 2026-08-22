// This plugin exposes no invoke commands — it only bridges iOS lifecycle
// notifications into Rust — so the command list is empty.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .ios_path("ios")
        .android_path("android")
        .build();
}
