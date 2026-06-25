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

## Features

| Command | What it does |
| --- | --- |
| `velox info` | CPU / GPU / battery / temperature overview |
| `velox thermals [--watch]` | Real-time temperature polling |
| `velox gpu status` | GPU usage / VRAM / temp (via `nvidia-smi`, WMI fallback) |
| `velox bench cpu\|gpu\|everyday\|all` | Single + multi-thread CPU benchmark, everyday workload, GPU monitor |
| `velox fps [--seconds N]` | Per-process frame detection via ETW (DXGI) |
| `velox drivers` | Read-only scan for problem devices (yellow-bang) |
| `velox doctor` | **One command** — scans everything, AI gives a full diagnosis |
| `velox diagnose [--fix]` | AI proposes a safe, reversible action; applies it on approval |
| `velox checkpoint save\|list\|restore` | Snapshot a known-good state; roll back later |
| `velox daemon [--interval N] [--auto]` | Background monitor; fires the AI loop on anomaly |
| `velox chorus ask "..." [--use M]` | Ask Claude / GPT / Gemini / Grok (auto-routed) |

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
  using independent models to cross-check before anything runs.
- **Always reversible.** A checkpoint is saved automatically before any change.
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

## Status (v0.5.0)

**Working & verified:** info, thermals, gpu status, bench, fps, drivers, doctor (live AI),
checkpoint save/list, daemon loop, chorus routing.

**Implemented, full end-to-end verification in progress:** the AI action loop
(`diagnose --fix`, `daemon --auto`) and the 3-stage pipeline fire on a real thermal
anomaly + Administrator rights. A **simulation mode** to force-trigger and prove the
complete loop without real heat is the next milestone.

**Out of scope (by design):** automated BIOS changes — non-reversible / can brick
hardware. Read-only at most.

---

## About

A high-school engineering project and the foundation of APEX — built to learn systems,
Rust, and AI orchestration by shipping real, reversible tools rather than demos.
