<div align="center">
  <img src="docs/assets/hero-banner.png" width="100%" alt="SolStream hero banner" />
</div>

<div align="center">

[![License: MIT](https://img.shields.io/badge/License-MIT-14f195?style=flat-square)](LICENSE)
[![Solana](https://img.shields.io/badge/Solana-4.x-9945ff?style=flat-square&logo=solana)](https://solana.com)
[![Rust](https://img.shields.io/badge/Rust-native_program-orange?style=flat-square&logo=rust)](program/)
[![TypeScript](https://img.shields.io/badge/TypeScript-SDK_%2B_dashboard-3178c6?style=flat-square&logo=typescript)](sdk/)

**Payment streaming on Solana.** Lock SPL tokens into a stream, vest them
linearly per second, withdraw as they unlock. Cancel anytime; both sides
settle pro-rata.

</div>

## Features

- **Linear vesting** with per-second rates and optional cliff
- **PDA vaults**: tokens live in a program-owned vault, moved only by
  program-signed CPIs
- **Checked math everywhere**: overflow fails the instruction, never wraps
- **Clean settlement**: cancel pays the recipient what vested, returns the
  rest, and closes both accounts
- **TypeScript SDK** with PDA derivation, instruction builders, and stream
  queries
- **React dashboard** with live vested balances, withdraw, and cancel

## Architecture

![Architecture](docs/architecture.svg)

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full design,
account layouts, and trust notes.

## Quickstart

Prerequisites: Rust toolchain, Node 18+, and the Solana CLI
(`solana`, `solana-test-validator`, `cargo-build-sbf`).

```bash
# Full end-to-end run: starts a local validator, builds and deploys the
# program, creates a test mint, and runs a stream lifecycle
# (create, partial withdraw, cancel) through the SDK.
./demo.sh
```

The SDK and dashboard:

```bash
cd sdk && npm install && npm run build
cd dashboard && npm install && npm run build   # then npm run dev
```

Note: `demo.sh` prefers a 2.1.x validator when one is installed next to the
workspace (the 4.x validator needs the `io_uring` syscall, which some
sandboxes block). If your sandbox also blocks UDP `sendto()`, the script
builds and preloads `scripts/udp_sendto_shim.c`, a tiny shim that keeps the
validator's QUIC traffic flowing. On a normal machine neither is needed.

## Instructions

| Instruction | Signer | Effect |
|---|---|---|
| `CreateStream` | sender | Validate schedule, open vault, fund it |
| `Withdraw` | recipient | Pay out newly vested tokens |
| `CancelStream` | sender | Settle both sides, close accounts |

`CreateStream` requires `rate_per_sec * (end - start) == amount`.
`Withdraw` before the cliff fails. `CancelStream` pays the recipient
`vested - withdrawn` and returns `deposited - vested` to the sender.

## Repository layout

```
program/     native Rust program (no framework) + program-test suite
sdk/         TypeScript SDK and lifecycle demo script
dashboard/   React + Vite dashboard
docs/        architecture doc, diagrams, assets
demo.sh      end-to-end demo against solana-test-validator
```

## Limitations

- Tested on `solana-test-validator` only. Not audited, not for real funds.
- Time comes from the clock sysvar, so vesting has slot-level granularity.
- Token support is the classic SPL Token program.

## License

MIT. See [LICENSE](LICENSE).
