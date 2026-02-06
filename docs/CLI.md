# CLI Reference

## Command Structure

- Top-level: `clmm_tool <command> [options]`
- RPC group: `clmm_tool rpc <subcommand> [options]`

## Common Conventions

- Token names are referred to as Token 0 (t0) and Token 1 (t1).
- Raw prices are ratios without decimal adjustment.
- Human prices are adjusted by mint decimals.
- Percent inputs (e.g., `--impact-pct`) are percentages. Example: `0.5` means 0.5%.

## Price Format Values

These are accepted where a price format is required:

- `t1-per-t0-raw`
- `t0-per-t1-raw`
- `t1-per-t0-human`
- `t0-per-t1-human`

For human-only formats (used by RPC commands):

- `t0-per-t1`
- `t1-per-t0`

## Offline Commands

### `tick-to-price`

Converts a tick index to raw and human price formats.

Usage:

```
clmm_tool tick-to-price --tick <TICK> --decimals0 <DECIMALS> --decimals1 <DECIMALS>
```

Options:

- `--tick <i32>`: Tick index to convert.
- `--decimals0 <u8>`: Token 0 mint decimals.
- `--decimals1 <u8>`: Token 1 mint decimals.

Output:

- Raw `t1-per-t0` and `t0-per-t1`.
- Human `t1-per-t0` and `t0-per-t1`.
- `sqrt_price_x64` derived from the tick.

### `price-to-tick`

Converts a price in a specified format to a tick index (rounded down).

Usage:

```
clmm_tool price-to-tick --decimals0 <DECIMALS> --decimals1 <DECIMALS> <FORMAT> <PRICE>
```

Options:

- `--decimals0 <u8>`: Token 0 mint decimals.
- `--decimals1 <u8>`: Token 1 mint decimals.
- `<FORMAT>`: One of the price formats listed above.
- `<PRICE>`: The price value in the chosen format.

### `tick-info`

Displays the tick array start index and slot for a given tick.

Usage:

```
clmm_tool tick-info --tick <TICK> --tick-spacing <SPACING>
```

Options:

- `--tick <i32>`: Tick index to inspect.
- `--tick-spacing <u16>`: Pool tick spacing.

### `array-info`

Displays the tick range covered by a tick array and the tick at each slot.

Usage:

```
clmm_tool array-info --start-index <INDEX> --tick-spacing <SPACING>
```

Options:

- `--start-index <i32>`: Tick array start index.
- `--tick-spacing <u16>`: Pool tick spacing.

### `array-to-price-range`

Converts a tick array range to prices in all formats.

Usage:

```
clmm_tool array-to-price-range --start-index <INDEX> --tick-spacing <SPACING> --decimals0 <DECIMALS> --decimals1 <DECIMALS>
```

Options:

- `--start-index <i32>`: Tick array start index.
- `--tick-spacing <u16>`: Pool tick spacing.
- `--decimals0 <u8>`: Token 0 mint decimals.
- `--decimals1 <u8>`: Token 1 mint decimals.

### `price-range-to-arrays`

Calculates all tick arrays crossed by a price range.

Usage:

```
clmm_tool price-range-to-arrays \
  --price-lower <PRICE> \
  --price-upper <PRICE> \
  --tick-spacing <SPACING> \
  --decimals0 <DECIMALS> \
  --decimals1 <DECIMALS> \
  --format <FORMAT>
```

Options:

- `--price-lower <f64>`: Lower price bound.
- `--price-upper <f64>`: Upper price bound.
- `--tick-spacing <u16>`: Pool tick spacing.
- `--decimals0 <u8>`: Token 0 mint decimals.
- `--decimals1 <u8>`: Token 1 mint decimals.
- `--format <FORMAT>`: Price format for inputs. Default is `t1-per-t0-human`.

### `derive-pda`

Derives the tick array PDA for a given pool and either a tick index or a price.

Usage (tick input):

```
clmm_tool derive-pda \
  --pool-id <POOL_ID> \
  --tick-spacing <SPACING> \
  --decimals0 <DECIMALS> \
  --decimals1 <DECIMALS> \
  --tick <TICK>
```

Usage (price input):

```
clmm_tool derive-pda \
  --pool-id <POOL_ID> \
  --tick-spacing <SPACING> \
  --decimals0 <DECIMALS> \
  --decimals1 <DECIMALS> \
  <FORMAT> <PRICE>
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--tick-spacing <u16>`: Pool tick spacing.
- `--decimals0 <u8>`: Token 0 mint decimals.
- `--decimals1 <u8>`: Token 1 mint decimals.
- `--tick <i32>`: Tick index input.
- `<FORMAT> <PRICE>`: Price input using one of the supported formats.

## RPC Commands

All RPC commands accept `--rpc-url <URL>` and default to `https://api.mainnet-beta.solana.com` if omitted.

### `rpc pool-state`

Fetches and parses the pool state account.

Usage:

```
clmm_tool rpc pool-state --pool-id <POOL_ID> [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc token-mints`

Fetches the Token 0 and Token 1 mint addresses.

Usage:

```
clmm_tool rpc token-mints --pool-id <POOL_ID> [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc default-bitmap`

Reads the default bitmap from the pool state to list initialized arrays in the central range.

Usage:

```
clmm_tool rpc default-bitmap --pool-id <POOL_ID> [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc extension-bitmap`

Reads the bitmap extension account to list initialized arrays outside the default range.

Usage:

```
clmm_tool rpc extension-bitmap --pool-id <POOL_ID> [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc tick-array`

Fetches and parses a specific tick array by start index.

Usage:

```
clmm_tool rpc tick-array --pool-id <POOL_ID> --start-index <INDEX> [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--start-index <i32>`: Tick array start index.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc full-analysis`

Fetches pool state and bitmaps, then prints all initialized arrays and the current price location.

Usage:

```
clmm_tool rpc full-analysis --pool-id <POOL_ID> [--format <t0-per-t1|t1-per-t0>] [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--format <t0-per-t1|t1-per-t0>`: Price display format. Default is `t0-per-t1`.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc liquidity-curve`

Fetches all initialized arrays and renders a liquidity distribution chart.

Usage:

```
clmm_tool rpc liquidity-curve \
  --pool-id <POOL_ID> \
  [--format <t0-per-t1|t1-per-t0>] \
  [--max-width <WIDTH>] \
  [--show-arrays] \
  [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--format <t0-per-t1|t1-per-t0>`: Price display format. Default is `t0-per-t1`.
- `--max-width <usize>`: Maximum bar width in characters. Default is `50`.
- `--show-arrays`: Show array start/end markers in the output.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc initialized-range`

Fetches initialized arrays within a price range and the nearest surrounding arrays.

Usage:

```
clmm_tool rpc initialized-range \
  --pool-id <POOL_ID> \
  --price-lower <PRICE> \
  --price-upper <PRICE> \
  --format <t0-per-t1|t1-per-t0> \
  [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--price-lower <f64>`: Lower price bound.
- `--price-upper <f64>`: Upper price bound.
- `--format <t0-per-t1|t1-per-t0>`: Price format for inputs.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc initialized-range-percent`

Fetches initialized arrays within a percentage band around a center price.

Usage:

```
clmm_tool rpc initialized-range-percent \
  --pool-id <POOL_ID> \
  --price <PRICE> \
  --lower-pct <PERCENT> \
  --upper-pct <PERCENT> \
  --format <t0-per-t1|t1-per-t0> \
  [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--price <f64>`: Center price for the range.
- `--lower-pct <f64>`: Lower percentage below the center price.
- `--upper-pct <f64>`: Upper percentage above the center price.
- `--format <t0-per-t1|t1-per-t0>`: Price format for inputs.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc get-swap-arrays`

Calculates required tick arrays for a swap using on-chain bitmap data.

Usage:

```
clmm_tool rpc get-swap-arrays \
  --pool-id <POOL_ID> \
  --direction <buy-t1|buy-t0> \
  --format <t0-per-t1|t1-per-t0> \
  --favorable-pct <PERCENT> \
  --impact-pct <PERCENT> \
  [--price <PRICE>] \
  [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--direction <buy-t1|buy-t0>`: Swap direction.
- `--format <t0-per-t1|t1-per-t0>`: Price format for `--price`.
- `--favorable-pct <f64>`: Maximum favorable move percentage.
- `--impact-pct <f64>`: Maximum adverse move percentage.
- `--price <f64>`: Optional starting price. If omitted, the current pool price is used.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc get-swap-arrays-blind`

Calculates required tick arrays for a swap without checking initialization.

Usage:

```
clmm_tool rpc get-swap-arrays-blind \
  --pool-id <POOL_ID> \
  --direction <buy-t1|buy-t0> \
  --format <t0-per-t1|t1-per-t0> \
  --favorable-pct <PERCENT> \
  --impact-pct <PERCENT> \
  [--price <PRICE>] \
  [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--direction <buy-t1|buy-t0>`: Swap direction.
- `--format <t0-per-t1|t1-per-t0>`: Price format for `--price`.
- `--favorable-pct <f64>`: Maximum favorable move percentage.
- `--impact-pct <f64>`: Maximum adverse move percentage.
- `--price <f64>`: Optional starting price. If omitted, the current pool price is used.
- `--rpc-url <string>`: RPC endpoint URL.

### `rpc inspect-array`

Fetches and renders a tick array by start index or PDA.

Usage:

```
clmm_tool rpc inspect-array --pool-id <POOL_ID> (--start-index <INDEX> | --pda <PDA>) [--rpc-url <URL>]
```

Options:

- `--pool-id <pubkey>`: Pool account address.
- `--start-index <i32>`: Tick array start index. Mutually exclusive with `--pda`.
- `--pda <pubkey>`: Tick array PDA. Mutually exclusive with `--start-index`.
- `--rpc-url <string>`: RPC endpoint URL.
