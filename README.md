# APEX Velox

> **Build Systems. Connect Everything.**
> A Rust command-line tool that reads your machine and lets AI reason about it — safely.

APEX Velox is a Windows system-telemetry and AI-orchestration CLI written in Rust.
It is the first pillar of **APEX**, a long-term project to connect hardware, the OS, and
AI into one coherent system.

```
APEX
├── Velox    ← system / telemetry / CLI  (this repo)
├── Chorus   ← multi-AI orchestration & routing
├── Core     ← runtime / automation hub  (future)
└── Pulse    ← UI                         (future)
```

---

## Architecture

A Cargo workspace that separates the **engine** from the **interface**:

```
velox-core/  (lib)  — engine: util · watch · ai · checkpoint · action   (returns data)
velox-cli/   (bin)  — interface: commands · output · interactive approval
```

`velox-core` is a reusable library — a future GUI (Pulse) or plugins import the same
engine. This is the "features → platform" transition, done by incremental extraction
(see [velox-core/README.md](velox-core/README.md)). Principle: **engine returns data,
the caller presents.**

---

## Features

| Command | What it does |
| --- | --- |
| `velox info` | CPU / GPU / battery / temperature overview |
| `velox snapshot [--json]` | One-shot system state (power plan / CPU / temp); `--json` emits machine-readable **engine data** |
| `velox thermals [--watch]` | Real-time temperature polling |
| `velox gpu status` | GPU usage / VRAM / temp (via `nvidia-smi`, WMI fallback) |
| `velox bench cpu\|stability\|everyday\|gpu\|all` | CPU score, **sustained/throttle test** (retention %), everyday workload, GPU monitor |
| `velox timeline record\|show` | Track performance over time vs your **personal best** (100%) |
| `velox fps [--seconds N]` | Per-process frame detection via ETW (DXGI) |
| `velox tempcheck [--seconds N]` | Monitor temp (max/min/avg, time ≥85°C) → AI thermal advice (read-only) |
| `velox fpscheck [--seconds N]` | Measure FPS (avg / 1% low) → AI game-performance advice (read-only) |
| `velox drivers [--analyze]` | Scan problem devices; `--analyze` adds AI driver-health advice |
| `velox doctor` | **One command** — scans everything, AI gives a full diagnosis |
| `velox diagnose [--fix] [--simulate-hot]` | AI proposes a safe, reversible action; applies it on approval |
| `velox checkpoint save\|list\|restore` | Snapshot a known-good state; roll back later |
| `velox daemon [--interval N] [--auto]` | Background monitor; fires the AI loop on a *sustained* anomaly |
| `velox chorus ask\|models\|set\|add\|test\|bench\|consensus` | Multi-AI: **semantic** auto-routing · add custom providers (OpenRouter/Ollama) · set keys · verify · model **benchmark** (multi-judge) · **consensus** |

---

## The AI safety model

Velox lets AI *act* on the machine — but inside hard safety rails. Every action runs
through this loop:

```
read → AI proposes → whitelist check → Confirmer AI → human approval → checkpoint → execute → verify → rollback
```

- **AI selects, never generates.** The AI can only choose from a hardcoded whitelist of
  safe, reversible actions — it never produces raw commands. Hallucination or prompt
  injection can't escalate into arbitrary execution.
- **3-stage AI** (`diagnose`): Customer (Claude) → Engineer (GPT) → Confirmer (Gemini),
  independent models cross-checking before anything runs.
- **Reality check.** Inputs are validated for physical plausibility first (e.g. 95°C while
  the CPU is idle ⇒ suspected sensor/bug ⇒ no action) — garbage in won't cause action.
- **Persistence.** The daemon acts only on a *sustained* anomaly (N consecutive reads),
  so a one-off glitch can't trigger anything.
- **Always reversible.** A checkpoint is auto-saved before any change; `checkpoint restore`
  reverts to the last good state.
- **Test ≠ real.** `--simulate-hot` is strictly dry-run: it exercises the full AI pipeline
  but never changes real state.
- **Human in the loop** by default (`--fix` + confirmation).

---

## Install

```bash
cargo build --release
# binary: target/release/velox.exe
```

### Setup (for AI features)

Copy `.env.example` to `.env` and fill in your keys (dotenv format — `KEY=value`):

```
ANTHROPIC_API_KEY=...
OPENAI_API_KEY=...
GEMINI_API_KEY=...
GROK_API_KEY=...
```

> Some features (temperature sensors, ETW frame detection) require an **Administrator**
> terminal.

---

## Examples

```bash
velox doctor                     # "why is my PC slow?" → full AI diagnosis
velox diagnose --fix             # AI proposes a fix, you approve, it applies + verifies
velox daemon --interval 30       # watch in the background, propose on anomaly
velox chorus ask "explain this Rust error" --use claude
```

---

## Tech stack

Rust · WMI · `sysinfo` · ETW (DXGI) · `nvidia-smi` · `powercfg` · `wevtutil` ·
Claude / GPT / Gemini / Grok APIs · `clap` · `tokio` · `reqwest`

---

## Status (v0.9.0)

**Core extraction:** the project is now a workspace — the engine (`velox-core`:
util · watch · ai · checkpoint · action) is split from the CLI (`velox-cli`), so the
same engine can back a future GUI / plugins.

**Verified end-to-end:** the full AI action loop is proven via `diagnose --simulate-hot` —
Customer (Claude) → Engineer (GPT) → Confirmer (Gemini APPROVE) → auto-checkpoint →
execute (power plan) → verify → `checkpoint restore`. The complete safety stack
(whitelist · 3-stage AI · reality check · persistence · dry-run · checkpoint · approval ·
cooldown) is in place.

**Performance suite:** `bench` (incl. sustained/throttle `stability`) + `timeline`
(track regressions vs your personal best). **Advisory tools** (read-only, AI advice):
`doctor`, `tempcheck`, `fpscheck`, `drivers --analyze`. **AI management:** `chorus
set` / `test` to add your own keys and verify every provider.

**Needs Administrator:** temperature sensors and ETW frame detection (`tempcheck`,
`fpscheck`, `fps`, real `diagnose --fix` on heat).

**Out of scope (by design):** automated BIOS changes — non-reversible / can brick
hardware. Read-only at most.

---

## Roadmap

- **v0.8 — Chorus provider architecture ✅:** custom providers (`chorus add`, OpenAI-
  compatible — OpenRouter, local Ollama, custom endpoints), **semantic routing**, multi-AI
  **consensus**, and a multi-judge **model benchmark** (`chorus bench`, 0–1000 scale).
- **v0.9 — Core extraction ✅ / smarter automation:** workspace split (`velox-core`
  engine + `velox-cli` interface) ✅; next: offline/online auto-switch, startup &
  background optimizer (a new *action* behind the whitelist + safety stack).
- **v1.0 — Stable release:** config file, installer, docs, **code signing** (so Smart App
  Control users can run it), polished UX. First public CLI.
- **Beyond (APEX Core / Pulse):** `velox-core` grows into a long-running service +
  plugin system; Pulse GUI (Tauri + React) imports the same engine.

---

## About

A high-school engineering project and the foundation of APEX — built to learn systems,
Rust, and AI orchestration by shipping real, reversible tools rather than demos.
