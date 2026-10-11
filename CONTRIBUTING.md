# Contributing

Thanks for helping improve Teletype. Before opening a pull request, please
check existing issues and keep changes focused on one problem.

## Development setup

Teletype's supported runtime target is macOS 15 or later on Apple Silicon.
Windows currently has a compile check but is not ready for end-user dictation.

Install Rust 1.90+, Node.js 20+, CMake, and a C/C++ toolchain. On macOS, install
the Xcode Command Line Tools with `xcode-select --install`. Then install the
UI dependencies and run the checks used by CI:

```bash
cd ui && npm ci && npx tsc --noEmit && npm run build
cd ..
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

To build and launch the desktop app, install the Tauri CLI and follow the
[Build and run](README.md#build-and-run) instructions in the README. A full
build compiles native speech dependencies and can take several minutes on the
first run.

## Pull requests

- Explain the user-visible behavior or bug being addressed.
- Include a regression test for behavior changes where practical.
- Update user-facing docs when setup, platform support, privacy behavior, or
  model availability changes.
- Do not add model weights, build outputs, credentials, or private user data.
- Confirm the commands above pass, or list any environment-specific check you
  could not run.
