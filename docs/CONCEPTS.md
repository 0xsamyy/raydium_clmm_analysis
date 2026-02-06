# Concepts

## Tokens and Price Formats

Raydium CLMM pools define two token mints: Token 0 and Token 1. This tool refers to them as `t0` and `t1`.

Price formats used by the CLI:

- `t1-per-t0-raw`: Raw ratio of token1/token0 with no decimal adjustment.
- `t0-per-t1-raw`: Raw ratio of token0/token1 with no decimal adjustment.
- `t1-per-t0-human`: Human-readable token1/token0 adjusted by mint decimals.
- `t0-per-t1-human`: Human-readable token0/token1 adjusted by mint decimals.

Decimal adjustment applied by the tool:

- `human = raw * 10^decimals0 / 10^decimals1`

## Ticks

A tick index is a discrete price point. The raw price is derived from the tick as:

- `raw_price = 1.0001 ^ tick`

Ticks are only valid at multiples of the pool's tick spacing.

## Tick Arrays

Tick arrays are on-chain accounts that store data for 60 valid ticks.

- Array size: 60 ticks
- Start index: multiple of `60 * tick_spacing`
- Slot index: `0..59`
- Tick index for a slot: `start_index + slot * tick_spacing`

## Bitmaps

The pool state contains a default bitmap of initialized arrays centered around the current price range. The extension bitmap accounts provide coverage for arrays outside the default range.

This tool reads both to discover which arrays are initialized.

## PDAs

Tick array PDAs are derived from:

- Seed `"tick_array"`
- Pool pubkey
- Array start index (big-endian i32)

Bitmap extension PDA is derived from:

- Seed `"pool_tick_array_bitmap_extension"`
- Pool pubkey

## Liquidity Distribution

The liquidity curve output aggregates tick liquidity net values into cumulative ranges and renders a text chart. The chart width is scaled by `--max-width`.

## Precision Notes

All math uses `f64`. Very large ticks or extreme prices can overflow or underflow. Use caution when working at the protocol limits.
