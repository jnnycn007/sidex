# SIDEN

**Research Company**

---

## Who We Are

Siden is a research company building infrastructure for how developers find, access, and work with code.

Two products. One mission: **make developer tools lighter, faster, and smarter.**

- **siden.ai**
- kendall@siden.ai

---

## Team

| | |
|---|---|
| **Kendall Booker** | CEO & Co-Founder |
| **Dragos Hornoiu** | CTO & Co-Founder |

---

## Products

### SideX

**VSCode's workbench, without Electron.**

A full port of Visual Studio Code that replaces Electron with Tauri 2 — a Rust backend and the OS's native webview. Same editor. Same extensions. ~50x less memory.

| | VS Code | SideX |
|---|---|---|
| RAM at idle | 797 MB | 16 MB |
| Runtime | Bundled Chromium | OS native webview |
| Disk | ~350 MB | ~50 MB |

- Monaco editor, integrated terminal, Git, 82 language extensions
- Cross-platform: macOS, Windows, Linux
- Open source (MIT)

### Sidera

**Search and retrieval infrastructure.**

Intelligent code search, indexing, and retrieval — built to power the next generation of developer tools and AI-assisted workflows.

---

## Problem

Developers waste resources on bloated tools and fragmented search.

- VS Code ships an entire Chromium browser just to render a text editor — **800 MB of RAM at idle**
- Code search is slow, siloed, and disconnected from the tools developers actually use
- Existing solutions force a choice: **lightweight or full-featured — never both**

---

## Solution

We build from the ground up in Rust and TypeScript, eliminating unnecessary overhead while keeping full compatibility with the ecosystems developers already depend on.

- **SideX** — full VS Code compatibility at a fraction of the resource cost
- **Sidera** — fast, intelligent search and retrieval across codebases

---

## Market

**2 billion potential users** — developers, startups, freelancers, and small businesses.

| | Size |
|---|---|
| **TAM** | $757B — Global software and developer tools |
| **SAM** | $110B — Code editors, search, and dev infrastructure |
| **SOM** | $3B — Lightweight editors, code search, and retrieval tools |

- 73M+ developers worldwide
- 53M+ use VS Code
- Developer tools market: $22B in 2025, projected $45B by 2030

---

## Tech

| Layer | Stack |
|---|---|
| Frontend | TypeScript, Vite 6, Monaco Editor |
| Backend | Rust, Tauri 2 |
| Terminal | portable-pty, xterm.js + WebGL |
| Search | dashmap + rayon + regex (parallel Rust) |
| Storage | SQLite via rusqlite |
| WASM | TF-IDF ranking, hashing, scroll computation |
| Extensions | Open VSX registry, custom WebSocket host |

---

## Traction

- Full VS Code workbench ported to Tauri 2
- 82 bundled language extensions
- Cross-platform builds shipping (macOS arm64/x64, Windows, Linux)
- Open source on GitHub (MIT)
- Extension host with LSP bridge in active development
- Community growing on Discord

---

## Roadmap

| When | What |
|---|---|
| Q2 2026 | SideX v0.2 — Full LSP, extension API parity |
| Q3 2026 | SideX v0.3 — Debugger, DAP integration |
| Q3 2026 | SideX v0.5 — Public beta, auto-updates |
| Q4 2026 | SideX v1.0 — Stable release |
| Q4 2026 | Sidera beta — Search and retrieval platform |

---

## What We Need

**AWS credits to build the infrastructure layer behind both products.**

| Service | Use |
|---|---|
| S3 + CloudFront | Binary distribution, CDN |
| EC2 / ECS | Search backend, extension proxy, APIs |
| RDS / DynamoDB | User data, index storage, analytics |
| Lambda | Serverless endpoints, webhooks |
| CloudWatch | Monitoring and logging |

**Estimated usage:** $1,000–2,000/mo

**Requesting:** $10,000–$25,000 in AWS Activate credits (12 months)

---

## Why Us

1. **Already built the hard part.** Full VS Code workbench running on Tauri — that's done.
2. **53M users paying a Chromium tax** they don't need to.
3. **Rust-native performance.** Parallel search, WASM computation, minimal overhead.
4. **Open source with a commercial path.** MIT drives adoption. Pro/Enterprise drives revenue.
5. **Capital efficient.** Our architecture means lower infra costs than any Electron-based competitor.

---

**Siden Research Company**

siden.ai

kendall@siden.ai
