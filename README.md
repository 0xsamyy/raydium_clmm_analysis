# Raydium CLMM Analysis Tool

A command-line tool for offline tick/price math and on-chain inspection of Raydium CLMM pools on Solana.

## Applications

- Validate tick and price conversions for integrations and testing.
- Inspect initialized tick arrays and their liquidity distribution.
- Plan swap array coverage for quotes or execution paths.
- Analyze liquidity coverage around a target price range.

## Requirements

- Rust toolchain (stable).
- A Solana RPC endpoint for `rpc` commands (defaults to mainnet).

## Installation

```bash
cargo build --release
```

## Quick Start

```bash
./target/release/clmm_tool tick-to-price --tick 0 --decimals0 6 --decimals1 6
./target/release/clmm_tool rpc pool-state --pool-id <POOL_ID>
```

## Documentation

- docs/CLI.md
- docs/CONCEPTS.md

## Notes

- All `rpc` commands are read-only and do not require private keys.
- Use `--rpc-url` to target a non-default endpoint.
