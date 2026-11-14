### Added
- WebAssembly plugin platform (Wasmtime, WIT package `ilium:plugin@0.1.0`) with a plugin host, SDK, test kit, conformance suite and sandboxed helper process.
- Core feature packages `ilium.core.*` are being moved onto the plugin platform under `plugins/core/` (30 packages so far; the move is not finished).
- Plugin manual with production sections covering installation, permissions, lifecycle, security, packaging and operations.
- Plugin requirement test suite with at least two tagged tests per testable requirement.
- Plugin scenes: a plugin's `ilium:anim` scene component draws through the host frame surface, and each scene it declares in `contributes.scenes` is registered with the client's external background scenes. Choosing one in the animation list is not wired yet.
