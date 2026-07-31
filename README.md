# APEX Velox

> **Build Systems. Connect Everything.**
> A Rust command-line tool that reads your machine and lets AI reason about it — safely.

APEX Velox is a Windows system-health and AI-development tool written in Rust.
It is the first pillar of **APEX**, a long-term project to connect hardware, the OS, and
AI into one coherent system.

```
APEX Velox
├── Pulse       ← native desktop interface
├── Local API   ← authenticated localhost bridge
├── Velox CLI   ← terminal interface / advanced tools
└── Velox Core  ← health · benchmark · snapshot · AI · safety engine
```

---

## Architecture

A Cargo workspace that separates the **engine** from the **interface**:

```
velox-core/    (lib) — engine: structured reports · AI · safety (returns data)
velox-cli/     (bin) — terminal interface · advanced commands · approval
velox-server/  (bin) — authenticated localhost API for Pulse
velox-app/     (bin) — native WebView2 desktop shell
site/                — shared web/app interface
```

`velox-core` is a reusable library — Pulse, the local API, CLI, and future plugins use the same
engine. The three primary product flows are **PC Health**, **CPU Benchmark**, and
**Snapshot/Compare**. This is the "features → platform" transition, done by incremental extraction
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
| `velox chorus ask\|models\|model\|consent\|set\|add\|test\|bench\|consensus` | Multi-AI **through the policy gateway**: per-provider **consent**/revoke · configurable **model** IDs · semantic routing · custom providers (OpenRouter/Ollama) · set/verify keys · model **benchmark** · **consensus** |

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
- **Cross-checking AI** (`diagnose` CLI): Customer → Engineer → Confirmer providers
  cross-check before anything runs. Pulse's read-only diagnosis currently uses Claude + GPT.
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

Use Settings in the desktop app or `velox chorus set <provider> <key>`. New keys are
stored in the operating system credential vault (Windows Credential Manager), never
returned by the local API, and only loaded in memory when a provider is called.

`.env` remains supported only as a development/migration fallback:

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

## Status (v0.18.0 RC — alpha)

**Read-only Project Intelligence (v0.18):** `velox project scan` and the Pulse Project room
inspect a selected local project without running commands or changing files. Secret files,
absolute paths, symlink escapes, and oversized input are blocked in Core. Optional Council
analysis receives only typed, redacted Project Evidence and remains recommendation-only.

**Local Usage Ledger (v0.17):** APEX records provider/model/token metadata locally and can
estimate API cost only when the user supplies a dated price. It never stores prompts, responses,
API keys, subscription balances, or invented prices. Corrupt ledgers are quarantined and
concurrent writers are serialized.

**Local Management Hub (v0.16):** Settings hosts a per-provider hub to manage API keys
(add / status / delete), cloud consent + data scope, and model IDs — all wired to the local
core APIs, keys staying in the Windows Credential Manager. No accounts or sync.

**Policy-enforced multi-AI foundation.** Every cloud-AI call in a product path goes through
one gateway (`execute_agent`) that enforces a per-provider **Agent Policy**: cloud calls are
**deny-by-default** until the user grants consent (per provider, with a maximum data scope),
and `query_text_with` is compile-locked `pub(crate)` so nothing bypasses it. AI payloads are
generated **only** from a typed, scope-validated `EvidenceBundle` — never a hand-built string.
A read-only **Council** (Claude proposes → GPT reviews → a deterministic APEX gate) turns
multiple AIs into a checked recommendation that **executes nothing**; every finding must cite
an EvidenceId present in the bundle.

Workspace crates share one version. Pulse starts its local server on a random loopback port
with a per-launch session token, verifies startup, and shuts the child server down with the app.
Health and CPU benchmark results come from structured `velox-core` reports shared by CLI and API.

**Privacy & consent:** cloud AI is off until you consent per provider (`chorus consent
<provider> [--scope minimal|system|drivers]`, or in-app before a diagnosis/Council run). The
payload is built from typed Evidence at or below the approved scope and previewed before it is
sent. Corrupt policy / evidence / model files fail closed.

**Safety boundary:** Pulse exposes structured read-only reports, dry-run diagnosis, and the
read-only Council (which returns a decision only — it never executes an action). Real system
actions remain in the CLI behind a whitelist, human approval, a power-plan checkpoint,
verification, and rollback. The checkpoint restores APEX-managed power-plan changes; it is not
a full Windows restore point. Executable, approved project/system actions are a later version.

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

Detailed engineering handoff and version gates: [CLAUDE_HANDOFF.md](CLAUDE_HANDOFF.md)

- **v0.8 — Chorus provider architecture ✅:** custom providers (`chorus add`, OpenAI-
  compatible — OpenRouter, local Ollama, custom endpoints), **semantic routing**, multi-AI
  **consensus**, and a multi-judge **model benchmark** (`chorus bench`, 0–1000 scale).
- **v0.9 — Core extraction ✅:** workspace split (`velox-core` engine + `velox-cli` interface).
- **v0.14 — Product core ✅:** unified versions; structured Health/Benchmark/Snapshot
  flows; native credential storage; minimum-data AI policy; authenticated random-port app bridge.
- **v0.15 — Policy-enforced multi-AI foundation ✅:** per-provider **Agent Policy** gateway
  (deny-by-default), cloud **consent** (CLI + in-app), typed **Evidence** engine, and a
  read-only **Council** (Claude → GPT → deterministic gate) surfaced in Pulse. All product
  AI payloads come from Evidence only. *(Real repair-workflow field test moves to v0.20.)*
- **v0.16 — Local Management Hub ✅:** one place in Settings to manage per-provider API keys
  (add / status / delete), cloud **consent** + data scope, and **model** IDs — wired to the
  local core APIs. Keys stay in the Windows Credential Manager. No accounts, sync, or session
  content.
- **v0.17 — Local Usage Ledger ✅:** local metadata-only AI call history, provider/model/token
  summaries, and explicitly labelled estimated API cost using user-configured dated prices.
  No prompt/response content, subscription balance, or fabricated default pricing.
- **v0.18 — Read-only Project Intelligence (RC):** secure project sessions, bounded scanning,
  secret/path redaction, typed Project Evidence, CLI/API/Pulse surfaces, and optional
  Claude → GPT Council analysis. No command execution or project writes.
- **v1.0 — Stable release:** config file, installer, docs, **code signing** (so Smart App
  Control users can run it), polished UX. First public CLI.
- **Beyond:** provider adapters for GPT/Claude/Ollama-compatible desktop models, consented
  context routing, and an agent/tool protocol grow on the same Core without bypassing policy.

---

## About

A high-school engineering project and the foundation of APEX — built to learn systems,
Rust, and AI orchestration by shipping real, reversible tools rather than demos.
