# velox-core

The shared **engine** behind APEX Velox. Pure logic — it returns data; the caller
(the CLI today, a GUI or plugins later) does the presenting.

## Modules

| Module | What it provides |
| --- | --- |
| `util` | console-output decoding (system codepage → UTF-8 fallback) |
| `watch` | live CPU / RAM / Disk / Network metrics (`Watcher`, `Metrics`) |
| `ai` | multi-provider AI calls + routing (`query_text_with`, `route_semantic`, custom providers) |
| `health` | deterministic, AI-independent PC health report |
| `benchmark` | structured CPU benchmark report shared by CLI/API |
| `privacy` | explicit minimum/system/driver AI context scopes |
| `credentials` | native OS credential-vault storage for provider keys |
| `checkpoint` | save / restore a known-good system state |
| `action` | whitelisted, reversible system actions (power plan) |

## Why a separate crate

APEX is a Cargo workspace: **`velox-core` (engine) + `velox-cli` (interface)**. The
same engine can back a CLI, a GUI (Pulse), or plugins — the "features → platform"
transition, done by incremental extraction.

```rust
use velox_core::ai;
use velox_core::watch::Watcher;

let answer = ai::query_text_with("claude", "hello").await;   // AI engine
let mut w = Watcher::new();
let m = w.read(1);                                            // CPU/RAM/Disk/Net
```

**Principle: the engine returns data; the caller presents.**
