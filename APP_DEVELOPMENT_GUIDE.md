# MoFA App Development Guide

This guide walks you through creating a new MofaApp plugin and using the
**runtime hot-reload** workflow to iterate quickly without restarting the shell.

---

## 1. Plugin crate setup

Create a new library crate for your plugin:

```bash
cargo new --lib crates/mofa-myplugin
```

In `crates/mofa-myplugin/Cargo.toml` set the crate type to `cdylib`:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
```

---

## 2. Implement the C-ABI entry point

Every plugin **must** export a `mofa_app_create` symbol with a stable C ABI:

```rust
#[no_mangle]
pub extern "C" fn mofa_app_create() -> *mut () {
    let app = Box::new(MyPlugin::new());
    Box::into_raw(app) as *mut ()
}

struct MyPlugin;

impl MyPlugin {
    fn new() -> Self { MyPlugin }
}
```

> **ABI stability:** Use `extern "C"` for all exported symbols.  Rust's default
> ABI is not stable across compiler versions.

---

## 3. Add a `plugin.toml` manifest

Place a `plugin.toml` next to the compiled shared library:

```toml
[plugin]
id      = "mofa-myplugin"
version = "0.1.0"
entry   = "libmofa_myplugin.so"
```

For a typical `cargo build --release` the library lands in
`target/release/libmofa_myplugin.so` (Linux) or `.dylib` (macOS).

Copy `plugin.toml` there, or use a build script to do it automatically.

---

## 4. Load plugins at shell startup

```rust
use mofa_plugin_loader::{load_all_plugins, PluginRegistry};
use std::path::Path;

let registry = PluginRegistry::new();
let plugin_dir = Path::new("plugins");

let n = load_all_plugins(plugin_dir, &registry);
println!("Loaded {n} plugins");
```

Each plugin lives in its own sub-directory inside `plugins/`:

```
plugins/
  mofa-fm/
    plugin.toml
    libmofa_fm.so
  mofa-settings/
    plugin.toml
    libmofa_settings.so
```

---

## 5. Enable hot-reload during development

Hot-reload is **off by default**.  Enable it with the environment variable:

```bash
MOFA_HOT_RELOAD=1 cargo run
```

When enabled, the shell:

1. Watches the plugin directory for `.so` / `.dylib` changes.
2. On change: drops the old `PluginHandle` (unloads the library).
3. Calls `PluginHandle::load()` to load the new version.
4. Re-registers the plugin in the `PluginRegistry`.
5. Calls your `on_reload` callback so the UI can refresh.

### Typical iteration loop

```bash
MOFA_HOT_RELOAD=1 cargo run -p mofa-studio-shell

cargo build -p mofa-myplugin
cp target/debug/libmofa_myplugin.so plugins/mofa-myplugin/
```

Or automate with `cargo-watch`:

```bash
cargo watch -w crates/mofa-myplugin/src \
  -s 'cargo build -p mofa-myplugin && \
      cp target/debug/libmofa_myplugin.so plugins/mofa-myplugin/'
```

---

## 6. Spawning the hot-reload thread

```rust
use mofa_plugin_loader::{hot_reload_enabled, spawn_hot_reload_thread};
use std::path::PathBuf;

if hot_reload_enabled() {
    let _guard = spawn_hot_reload_thread(
        PathBuf::from("plugins"),
        registry.clone(),
        |info| {
            println!("Reloaded plugin: {} v{}", info.id, info.version);
        },
    )
    .expect("failed to start hot-reload watcher");

}
```

> **Note:** Plugin state is **lost** on reload.  This is intentional for dev
> mode.  Production deployments should leave `MOFA_HOT_RELOAD` unset.

---

## 7. Safety considerations

| Concern | Mitigation |
|---|---|
| `unsafe` in `PluginHandle::load` | Contained inside the loader crate; plugin ABI contract is enforced by `extern "C"` |
| Use-after-free on reload | Old `PluginHandle` is dropped before the new one is registered; `Library` is kept alive by the handle |
| Production risk | Feature is fully disabled unless `MOFA_HOT_RELOAD=1` is explicitly set |
| Symbol name clashes | Each plugin exports a single well-known symbol; namespacing via plugin id prevents conflicts |

---

## 8. Platform notes

| Platform | Library extension | Status |
|---|---|---|
| Linux | `.so` | ✅ Supported |
| macOS | `.dylib` | ✅ Supported |
| Windows | `.dll` | 🚧 Planned |

The file watcher (`notify` crate) uses `inotify` on Linux and `FSEvents` on
macOS, both of which are zero-overhead in normal operation.
