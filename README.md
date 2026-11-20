# wgpu socket demo

A Rust demo that renders a 3D cube with **wgpu**, manages the window and event loop with **winit**, provides controls with **egui**, and synchronizes two windows through a **Unix domain socket**. It targets Linux and macOS.

## Run

Use two terminals on the same machine:

```sh
cargo run -- host
cargo run -- join
```

Adjust the rotation or scale sliders, or change the cube's color in either window. The change appears in both windows. The host orders updates and displays the current revision number. If the host closes, the join window retries the connection when the host starts again.

To use a different socket path, pass the same option to both processes:

```sh
cargo run -- host --socket /tmp/my-scene.sock
cargo run -- join --socket /tmp/my-scene.sock
```

A host accepts one join window at a time. It removes the socket file on a normal exit and recovers a stale socket with no active listener at startup.

## Checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
