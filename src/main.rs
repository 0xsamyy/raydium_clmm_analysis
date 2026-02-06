use solana_sdk::pubkey::Pubkey;
use solana_client::rpc_client::RpcClient;
use std::str::FromStr;
use clap::{Parser, Subcommand, ValueEnum};

use anchor_lang::AnchorDeserialize;

// --- Module Imports ---
mod onchain_states;
use onchain_states::{PoolState, TickArrayBitmapExtension, TickArrayState};

// --- Core Constants ---
const TICK_ARRAY_SIZE: i32 = 60;
const Q_RATIO: f64 = 1.0001;
const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
const TICK_ARRAY_SEED: &[u8] = b"tick_array";
const TICK_ARRAY_BITMAP_SEED: &[u8] = b"pool_tick_array_bitmap_extension";

// --- Data Structures for Clarity ---

/// Enum to define the various ways a price can be represented.
#[derive(Debug, Clone, Copy, Subcommand)]
enum PriceInput {
    /// Raw price ratio: token_1 / token_0 (no decimal adjustment)
    #[clap(name = "t1-per-t0-raw")]
    Token1PerToken0Raw { price: f64 },
    /// Raw price ratio: token_0 / token_1 (no decimal adjustment)
    #[clap(name = "t0-per-t1-raw")]
    Token0PerToken1Raw { price: f64 },
    /// Human-readable price: token_1 / token_0 (with decimal adjustment)
    #[clap(name = "t1-per-t0-human")]
    Token1PerToken0Human { price: f64 },
    /// Human-readable price: token_0 / token_1 (with decimal adjustment)
    #[clap(name = "t0-per-t1-human")]
    Token0PerToken1Human { price: f64 },
}

/// Helper struct for all tick-to-price and price-to-tick conversions.
struct TickConverter {
    decimals_0: u8,
    decimals_1: u8,
}

impl TickConverter {
    // --- Core Conversion Logic ---

    /// Converts a tick index to its raw price (token_1 / token_0).
    fn tick_to_raw_price(&self, tick: i32) -> f64 {
        Q_RATIO.powi(tick)
    }

    /// Converts a raw price (token_1 / token_0) to its corresponding tick index (by rounding down).
    fn raw_price_to_tick(&self, price: f64) -> i32 {
        price.log(Q_RATIO).floor() as i32
    }
    
    // --- Flexible Conversion Functions ---

    /// Converts a tick index to a price in any of the specified formats.
    fn tick_to_price(&self, tick: i32, format: PriceInput) -> f64 {
        let raw_price = self.tick_to_raw_price(tick);
        let decimal_adjustment = 10f64.powi(self.decimals_0 as i32) / 10f64.powi(self.decimals_1 as i32);
        
        match format {
            PriceInput::Token1PerToken0Raw { .. } => raw_price,
            PriceInput::Token0PerToken1Raw { .. } => 1.0 / raw_price,
            PriceInput::Token1PerToken0Human { .. } => raw_price * decimal_adjustment,
            PriceInput::Token0PerToken1Human { .. } => 1.0 / (raw_price * decimal_adjustment),
        }
    }

    /// Converts a price from any specified format back to a tick index.
    fn price_to_tick(&self, price_info: PriceInput) -> i32 {
        let decimal_adjustment = 10f64.powi(self.decimals_0 as i32) / 10f64.powi(self.decimals_1 as i32);

        let raw_price = match price_info {
            PriceInput::Token1PerToken0Raw { price } => price,
            PriceInput::Token0PerToken1Raw { price } => 1.0 / price,
            PriceInput::Token1PerToken0Human { price } => price / decimal_adjustment,
            PriceInput::Token0PerToken1Human { price } => 1.0 / (price * decimal_adjustment),
        };
        
        self.raw_price_to_tick(raw_price)
    }

    /// Prints all price variations for a given tick index.
    fn print_all_prices(&self, tick: i32) {
        println!("--- Price Representations for Tick Index {} ---", tick);

        let t1_per_t0_raw_price = self.tick_to_price(tick, PriceInput::Token1PerToken0Raw { price: 0.0 });
        println!("  - Token1/Token0 (Raw):   {:.12}", t1_per_t0_raw_price);
        
        let t0_per_t1_raw_price = self.tick_to_price(tick, PriceInput::Token0PerToken1Raw { price: 0.0 });
        println!("  - Token0/Token1 (Raw):   {:.12}", t0_per_t1_raw_price);

        let t1_per_t0_human_price = self.tick_to_price(tick, PriceInput::Token1PerToken0Human { price: 0.0 });
        println!("  - Token1/Token0 (Human): {:.12}", t1_per_t0_human_price);
        
        let t0_per_t1_human_price = self.tick_to_price(tick, PriceInput::Token0PerToken1Human { price: 0.0 });
        println!("  - Token0/Token1 (Human): {:.12}", t0_per_t1_human_price);

        let sqrt_price_x64 = (self.tick_to_raw_price(tick).sqrt() * (2_u128.pow(64) as f64)) as u128;
        println!("  - SqrtPriceX64:          {}", sqrt_price_x64);
    }
}

/// Helper struct for all logic related to tick arrays, slots, and indices.
struct TickArrayHelper {
    tick_spacing: u16,
}

impl TickArrayHelper {
    /// Calculates the total number of tick *indices* covered by one tick array.
    fn tick_indices_per_array(&self) -> i32 {
        TICK_ARRAY_SIZE * self.tick_spacing as i32
    }

    /// Gets the start tick index for the array that contains a given tick index.
    fn get_array_start_index(&self, tick_index: i32) -> i32 {
        let ticks_in_array = self.tick_indices_per_array();
        let mut start = tick_index / ticks_in_array;
        
        if tick_index < 0 && tick_index % ticks_in_array != 0 {
            start -= 1;
        }
        
        start * ticks_in_array
    }
    
    /// Given a start_tick_index, determines the full range of tick *indices* it covers.
    fn get_array_tick_range(&self, start_index: i32) -> (i32, i32) {
        let end_index = start_index + self.tick_indices_per_array();
        (start_index, end_index - 1)
    }
    
    /// Aligns a tick to be a valid tick according to the pool's tick spacing.
    fn align_tick_to_spacing(&self, tick: i32) -> i32 {
        let mut compressed = tick / self.tick_spacing as i32;
        if tick < 0 && tick % self.tick_spacing as i32 != 0 {
            compressed -= 1; // Round toward negative infinity for consistency
        }
        compressed * self.tick_spacing as i32
    }


    /// Prints detailed information about a specific tick index.
    fn print_tick_info(&self, tick_index: i32) {
        let aligned_tick = self.align_tick_to_spacing(tick_index);
        let start_index = self.get_array_start_index(aligned_tick);
        let offset = (aligned_tick - start_index) / self.tick_spacing as i32;

        println!("--- Info for Tick Index {} ---", tick_index);
        if tick_index != aligned_tick {
             println!("  - Note: This tick is not a valid boundary. The nearest valid tick is {}.", aligned_tick);
        }
        println!("  - Belongs to Tick Array starting at index: {}", start_index);
        println!("  - Located at Slot (offset) {} within that array.", offset);
        println!("  - Note: A 'Slot' is the storage position (0-59). A 'Tick Index' is the absolute price level.");
    }
    
    /// Prints detailed information about a specific tick array.
    fn print_array_info(&self, start_index: i32) {
        let (start, end) = self.get_array_tick_range(start_index);
        
        println!("--- Info for Tick Array starting at {} ---", start_index);
        println!("  - Tick Spacing: {}", self.tick_spacing);
        println!("  - Covers Tick Index Range: [{}, {}]", start, end);
        println!("  - Contains {} storage 'Slots'. Each slot holds data for one *valid* tick.", TICK_ARRAY_SIZE);
        println!("  - Slot to Tick Index Mapping:");

        for slot in 0..TICK_ARRAY_SIZE {
            let tick_index = start_index + (slot * self.tick_spacing as i32);
            println!("    - Slot {:2}: Tick {}", slot, tick_index);
        }
    }
}

/// --- CLI Argument Parsing ---
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert a tick index to various price formats.
    TickToPrice {
        #[clap(long)]
        tick: i32,
        #[clap(long)]
        decimals0: u8,
        #[clap(long)]
        decimals1: u8,
    },
    /// Convert a price (in various formats) to a tick index.
    PriceToTick {
        #[clap(long)]
        decimals0: u8,
        #[clap(long)]
        decimals1: u8,
        #[clap(subcommand)]
        price: PriceInput,
    },
    /// Get information about a tick array from its start index.
    ArrayInfo {
        #[clap(long)]
        start_index: i32,
        #[clap(long)]
        tick_spacing: u16,
    },
    /// Find which tick array and slot a specific tick index belongs to.
    TickInfo {
        #[clap(long)]
        tick: i32,
        #[clap(long)]
        tick_spacing: u16,
    },
    /// Convert a tick array to its corresponding price range.
    ArrayToPriceRange {
        #[clap(long)]
        start_index: i32,
        #[clap(long)]
        tick_spacing: u16,
        #[clap(long)]
        decimals0: u8,
        #[clap(long)]
        decimals1: u8,
    },
    /// Find all tick arrays that a given price range crosses.
    PriceRangeToArrays {
        #[clap(long)]
        price_lower: f64,
        #[clap(long)]
        price_upper: f64,
        #[clap(long)]
        tick_spacing: u16,
        #[clap(long)]
        decimals0: u8,
        #[clap(long)]
        decimals1: u8,
        #[clap(long, value_enum, default_value_t = ArgPriceFormat::T1PerT0Human)]
        format: ArgPriceFormat,
    },
    /// Derive the PDA for a tick array from a tick index or price.
    DerivePda {
        #[clap(long)]
        pool_id: String,
        #[clap(long)]
        tick_spacing: u16,
        #[clap(long)]
        decimals0: u8,
        #[clap(long)]
        decimals1: u8,
        #[clap(long)]
        tick: Option<i32>,
        #[clap(subcommand)]
        price: Option<PriceInput>,
    },
    /// --- New RPC Commands ---
    #[clap(subcommand)]
    Rpc(RpcCommands),
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ArgPriceFormat {
    T1PerT0Raw,
    T0PerT1Raw,
    T1PerT0Human,
    T0PerT1Human,
}

// --- New CLI Commands for RPC ---
#[derive(Subcommand)]
enum RpcCommands {
    /// Fetches and parses the main pool state account.
    PoolState {
        #[clap(long)]
        pool_id: String,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Fetches the Token 0 and Token 1 mint addresses for a pool.
    TokenMints {
        #[clap(long)]
        pool_id: String,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Fetches and reads the default bitmap from the pool state.
    DefaultBitmap {
         #[clap(long)]
        pool_id: String,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Fetches and reads the bitmap extension account.
    ExtensionBitmap {
         #[clap(long)]
        pool_id: String,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Fetches and parses a specific tick array account.
    TickArray {
         #[clap(long)]
        pool_id: String,
        #[clap(long)]
        start_index: i32,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Fetches pool state and all bitmaps to provide a full liquidity analysis.
    FullAnalysis {
        #[clap(long)]
        pool_id: String,
        #[clap(long, value_enum, default_value_t = HumanPriceFormat::T0PerT1)]
        format: HumanPriceFormat,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Displays a text-based visualization of the pool's liquidity distribution.
    LiquidityCurve {
        #[clap(long)]
        pool_id: String,
        #[clap(long, value_enum, default_value_t = HumanPriceFormat::T0PerT1)]
        format: HumanPriceFormat,
        #[clap(long, default_value = "50")]
        max_width: usize,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
        /// Show tick array start/end markers (debug mode)
        #[clap(long)]
        show_arrays: bool,
    },
    /// Fetches all *initialized* tick arrays within a given price range and their neighbors.
    InitializedRange {
        #[clap(long)]
        pool_id: String,
        #[clap(long)]
        price_lower: f64,
        #[clap(long)]
        price_upper: f64,
        /// The price format for your --price-lower and --price-upper inputs
        #[clap(long, value_enum)] 
        format: HumanPriceFormat,
        /// The RPC URL (uses the same default as other commands)
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Fetches initialized arrays based on a center price and percentage range.
    InitializedRangePercent {
        #[clap(long)]
        pool_id: String,
        /// The center price for the range.
        #[clap(long)]
        price: f64,
        /// The lower-end percentage (e.g., 10 for -10%).
        #[clap(long)]
        lower_pct: f64,
        /// The upper-end percentage (e.g., 30 for +30%).
        #[clap(long)]
        upper_pct: f64,
        /// The price format for your --price input.
        #[clap(long, value_enum)] 
        format: HumanPriceFormat,
        /// The RPC URL (uses the same default as other commands)
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Calculates the required tick arrays for a swap.
    GetSwapArrays {
        #[clap(long)]
        pool_id: String,
        /// Direction of the swap (e.g., 'buy-t1' or 'buy-t0').
        #[clap(long, value_enum)]
        direction: SwapDirection,
        /// Price format for the --price argument (e.g., 't0-per-t1').
        #[clap(long, value_enum)]
        format: HumanPriceFormat,
        /// Max % price move in your favor (for tx latency). e.g., 0.1
        #[clap(long)]
        favorable_pct: f64,
        /// Max % price move against you (for swap impact). e.g., 0.5
        #[clap(long)]
        impact_pct: f64,
        /// Optional: The price to start the calculation from.
        /// If not provided, uses the pool's live current price.
        #[clap(long)]
        price: Option<f64>,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Calculates the required tick arrays for a swap (blindly, assumes all arrays exist).
    GetSwapArraysBlind {
        #[clap(long)]
        pool_id: String,
        /// Direction of the swap (e.g., 'buy-t1' or 'buy-t0').
        #[clap(long, value_enum)]
        direction: SwapDirection,
        /// Price format for the --price argument (e.g., 't0-per-t1').
        #[clap(long, value_enum)]
        format: HumanPriceFormat,
        /// Max % price move in your favor (for tx latency). e.g., 0.1
        #[clap(long)]
        favorable_pct: f64,
        /// Max % price move against you (for swap impact). e.g., 0.5
        #[clap(long)]
        impact_pct: f64,
        /// Optional: The price to start the calculation from.
        /// If not provided, uses the pool's live current price.
        #[clap(long)]
        price: Option<f64>,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Fetches and visually inspects a single tick array by start index OR PDA.
    InspectArray {
        #[clap(long)]
        pool_id: String,
        /// The start tick index of the array to inspect (mutually exclusive with --pda)
        #[clap(long, group = "input")]
        start_index: Option<i32>,
        /// The PDA of the array to inspect (mutually exclusive with --start-index)
        #[clap(long, group = "input")]
        pda: Option<String>,
        #[clap(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum HumanPriceFormat {
    T0PerT1,
    T1PerT0,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum SwapDirection {
    /// Buying Token 1 by selling Token 0. Price/tick increases.
    #[clap(name = "buy-t1")]
    BuyT1,
    /// Buying Token 0 by selling Token 1. Price/tick decreases.
    #[clap(name = "buy-t0")]
    BuyT0,
}

// --- Liquidity Curve Helper Structs and Functions ---

fn format_liquidity(liquidity: u128) -> String {
    if liquidity >= 1_000_000_000_000 {
        format!("{:.2}T", liquidity as f64 / 1_000_000_000_000.0)
    } else if liquidity >= 1_000_000_000 {
        format!("{:.2}B", liquidity as f64 / 1_000_000_000.0)
    } else if liquidity >= 1_000_000 {
        format!("{:.2}M", liquidity as f64 / 1_000_000.0)
    } else if liquidity >= 1_000 {
        format!("{:.2}K", liquidity as f64 / 1_000.0)
    } else {
        format!("{}", liquidity)
    }
}

/// Prints a text-based visualization of the exact on-chain liquidity ranges.
fn print_exact_liquidity_ranges(
    all_ticks: &mut Vec<(i32, i128)>,
    converter: &TickConverter,
    price_format: PriceInput,
    max_width: usize,
    current_tick: i32,
    tick_spacing: u16,
    pool_pubkey: &Pubkey,
    program_id: &Pubkey,
    show_arrays: bool,
) {
    if all_ticks.is_empty() {
        println!("No liquidity boundaries found in this pool.");
        return;
    }

    all_ticks.sort_by_key(|(tick, _)| *tick);

    if all_ticks.len() == 1 {
        eprintln!(
            "Warning: only one initialized tick boundary found at tick {} (liq_net = {}). \
    This usually means the opposite boundary array wasn’t fetched due to a wrong PDA.",
            all_ticks[0].0, all_ticks[0].1
        );
    }

    // Find max cumulative liquidity for normalization
    let mut max_liquidity: i128 = 0;
    let mut temp_liquidity: i128 = 0;
    for &(_, liq_net) in all_ticks.iter() {
        temp_liquidity += liq_net;
        if temp_liquidity > max_liquidity {
            max_liquidity = temp_liquidity;
        }
    }

    if max_liquidity <= 0 {
        println!("No active liquidity found in this pool.");
        return;
    }

    println!("\n--- Exact Liquidity Distribution ---");
    println!("{:<35} | {:<12} | {}", "Price Range", "Liquidity", "Distribution");
    println!("{:-<100}", "");

    let ticks_per_array = TICK_ARRAY_SIZE * tick_spacing as i32;
    let mut cumulative_liquidity: i128 = 0;
    let mut last_tick_processed: Option<i32> = None;
    let mut current_array_start: Option<i32> = None;

    for &(tick, liquidity_net) in all_ticks.iter() {
        let array_start_index = (tick / ticks_per_array) * ticks_per_array;

        // Detect entering a new array
        if current_array_start != Some(array_start_index) {
            // If we were inside a previous array, close it
            if let Some(prev_start) = current_array_start {
                if show_arrays {
                    println!("--- Array End: {:<7} ---", prev_start + ticks_per_array - 1);
                }
            }

            // Open new array
            let (pda, _) = Pubkey::find_program_address(
                &[
                    TICK_ARRAY_SEED,
                    pool_pubkey.as_ref(),
                    &array_start_index.to_be_bytes(),
                ],
                program_id,
            );
            if show_arrays {
                println!(
                    "\n--- Array Start: {:<7} (PDA: {}) ---",
                    array_start_index, pda
                );
            }

            current_array_start = Some(array_start_index);
        }

        // Print the liquidity range (same as before)
        if let Some(last_tick) = last_tick_processed {
            if cumulative_liquidity > 0 {
                let price_start = converter.tick_to_price(last_tick, price_format);
                let price_end = converter.tick_to_price(tick - 1, price_format);
                let normalized =
                    (cumulative_liquidity as f64 / max_liquidity as f64 * max_width as f64) as usize;
                let bar = "█".repeat(normalized.max(1));

                let marker = if current_tick >= last_tick && current_tick < tick {
                    let current_price = converter.tick_to_price(current_tick, price_format);
                    format!("  [CURRENT PRICE: {:.6}]", current_price)
                } else {
                    String::new()
                };

                let (p_start, p_end) = if price_start < price_end {
                    (price_start, price_end)
                } else {
                    (price_end, price_start)
                };
                println!(
                    "[{:<15.6} - {:<15.6}] | {:<12} | {}{}",
                    p_start,
                    p_end,
                    format_liquidity(cumulative_liquidity as u128),
                    bar,
                    marker
                );
            }
        }

        cumulative_liquidity += liquidity_net;
        last_tick_processed = Some(tick);
    }

    // Close final array at the end
    if let Some(start) = current_array_start {
        if show_arrays {
            println!("--- Array End: {:<7} ---", start + ticks_per_array - 1);
        }
    }
}

/// Prints a visual representation of a single TickArrayState, highlighting initialized ticks.
fn print_tick_array_visualization(
    tick_array: &TickArrayState,
    tick_spacing: u16,
    pda: &Pubkey,
) {
    println!("\n--- Visual Inspection of Tick Array (Start Index: {}) ---", tick_array.start_tick_index);
    println!("PDA Address: {}", pda);
    println!("{} initialized ticks found.", tick_array.initialized_tick_count);
    println!("{:-<80}", "");

    for (slot_index, tick_state) in tick_array.ticks.iter().enumerate() {
        // Calculate the absolute tick index for this slot
        let tick_index = tick_array.start_tick_index + (slot_index as i32 * tick_spacing as i32);

        if tick_state.liquidity_gross != 0 {
            // This is an initialized tick, make it stand out
            println!("┌─ SLOT {:<2} ──────────────────────────────────────────────────────────────────┐", slot_index);
            println!("│  Tick Index: {}", tick_index);
            println!("│  Liquidity Net:   {}", tick_state.liquidity_net);
            println!("│  Liquidity Gross: {}", tick_state.liquidity_gross);
            println!("└──────────────────────────────────────────────────────────────────────────┘");
        } else {
            // This is an uninitialized tick
            println!("- Slot {:<2} (Tick {}) is empty.", slot_index, tick_index);
        }
    }
    println!("{:-<80}", "");
}

/// --- Main Application Logic ---
fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::TickToPrice { tick, decimals0, decimals1 } => {
            let converter = TickConverter { decimals_0: decimals0, decimals_1: decimals1 };
            converter.print_all_prices(tick);
        }
        Commands::PriceToTick { decimals0, decimals1, price } => {
            let converter = TickConverter { decimals_0: decimals0, decimals_1: decimals1 };
            let tick = converter.price_to_tick(price);
            println!("--- Price to Tick Conversion ---");
            println!("Input Price: {:?}", price);
            println!("Resulting Tick Index: {}", tick);
        }
        Commands::ArrayInfo { start_index, tick_spacing } => {
            let helper = TickArrayHelper { tick_spacing };
            helper.print_array_info(start_index);
        }
        Commands::TickInfo { tick, tick_spacing } => {
            let helper = TickArrayHelper { tick_spacing };
            helper.print_tick_info(tick);
        }
        Commands::ArrayToPriceRange { start_index, tick_spacing, decimals0, decimals1 } => {
            let helper = TickArrayHelper { tick_spacing };
            let converter = TickConverter { decimals_0: decimals0, decimals_1: decimals1 };
            let (tick_start, tick_end) = helper.get_array_tick_range(start_index);
            println!("--- Price Range for Tick Array {} ---", start_index);
            println!("\nStart of Range (Tick {}):", tick_start);
            converter.print_all_prices(tick_start);
            println!("\nEnd of Range (Tick {}):", tick_end);
            converter.print_all_prices(tick_end);
        }
        Commands::PriceRangeToArrays { price_lower, price_upper, tick_spacing, decimals0, decimals1, format } => {
            let converter = TickConverter { decimals_0: decimals0, decimals_1: decimals1 };
            let helper = TickArrayHelper { tick_spacing };
            
            // Determine which price format to use for the converter
            let (price_format_lower, price_format_upper, price_input_template) = match format {
                ArgPriceFormat::T1PerT0Raw => (
                    PriceInput::Token1PerToken0Raw { price: price_lower },
                    PriceInput::Token1PerToken0Raw { price: price_upper },
                    PriceInput::Token1PerToken0Raw { price: 0.0 }, // Template for later
                ),
                ArgPriceFormat::T0PerT1Raw => (
                    PriceInput::Token0PerToken1Raw { price: price_lower },
                    PriceInput::Token0PerToken1Raw { price: price_upper },
                    PriceInput::Token0PerToken1Raw { price: 0.0 },
                ),
                ArgPriceFormat::T1PerT0Human => (
                    PriceInput::Token1PerToken0Human { price: price_lower },
                    PriceInput::Token1PerToken0Human { price: price_upper },
                    PriceInput::Token1PerToken0Human { price: 0.0 },
                ),
                ArgPriceFormat::T0PerT1Human => (
                    PriceInput::Token0PerToken1Human { price: price_lower },
                    PriceInput::Token0PerToken1Human { price: price_upper },
                    PriceInput::Token0PerToken1Human { price: 0.0 },
                ),
            };

            let tick_lower = converter.price_to_tick(price_format_lower);
            let tick_upper = converter.price_to_tick(price_format_upper);

            let start_array_index = helper.get_array_start_index(tick_lower);
            let end_array_index = helper.get_array_start_index(tick_upper);
            
            println!("--- Arrays Crossed by Price Range [{:.6}, {:.6}] (Format: {:?}) ---", price_lower, price_upper, format);
            println!("  - Corresponding Tick Range: [{}, {}]", tick_lower, tick_upper);
            println!("\n{:<15} | {:<25} | {}", "Array Start", "Tick Range", "Price Range (in specified format)");
            println!("{:-<90}", "");

            let step = helper.tick_indices_per_array();

            // Handle both increasing and decreasing tick traversals
            if start_array_index <= end_array_index {
                let mut current_array_start = start_array_index;
                while current_array_start <= end_array_index {
                    let (tick_start, tick_end) = helper.get_array_tick_range(current_array_start);
                    let price_start = converter.tick_to_price(tick_start, price_input_template);
                    let price_end = converter.tick_to_price(tick_end, price_input_template);
                    
                    println!("{:<15} | [{:<11}, {:<11}] | [{:.8}, {:.8}]", current_array_start, tick_start, tick_end, price_start, price_end);
                    current_array_start += step;
                }
            } else { // Handle case where ticks are decreasing
                let mut current_array_start = start_array_index;
                while current_array_start >= end_array_index {
                    let (tick_start, tick_end) = helper.get_array_tick_range(current_array_start);
                    let price_start = converter.tick_to_price(tick_start, price_input_template);
                    let price_end = converter.tick_to_price(tick_end, price_input_template);

                    println!("{:<15} | [{:<11}, {:<11}] | [{:.8}, {:.8}]", current_array_start, tick_start, tick_end, price_start, price_end);
                    current_array_start -= step;
                }
            }
        }
        Commands::DerivePda { pool_id, tick_spacing, decimals0, decimals1, tick, price } => {
            if tick.is_none() && price.is_none() {
                eprintln!("Error: You must provide either --tick or a price subcommand for derive-pda.");
                return;
            }
             if tick.is_some() && price.is_some() {
                eprintln!("Error: You must provide either --tick OR a price subcommand, not both.");
                return;
            }

            let helper = TickArrayHelper { tick_spacing };
            let converter = TickConverter { decimals_0: decimals0, decimals_1: decimals1 };

            let input_tick = if let Some(t) = tick {
                t
            } else { // price must be Some
                converter.price_to_tick(price.unwrap())
            };

            let start_index = helper.get_array_start_index(input_tick);

            let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
            let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();
            
            let (pda, _bump) = Pubkey::find_program_address(
                &[
                    TICK_ARRAY_SEED,
                    pool_pubkey.as_ref(),
                    &start_index.to_be_bytes(),
                ],
                &program_id,
            );

            println!("--- Tick Array PDA Derivation ---");
            println!("  - Input Tick Index: {}", input_tick);
            println!("  - Tick Spacing: {}", tick_spacing);
            println!("  - This tick belongs to the array that *starts* at index: {}", start_index);
            println!("  - Pool ID: {}", pool_id);
            println!("  - Derived PDA: {}", pda);
        }
        Commands::Rpc(rpc_command) => {
            match rpc_command {
                RpcCommands::PoolState { pool_id, rpc_url } => {
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let account_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    
                    let pool_state = PoolState::deserialize(&mut &account_data[8..]).expect("Failed to parse pool state");

                    println!("--- Pool State for {} ---", pool_id);
                    println!("  - Liquidity: {}", pool_state.liquidity);
                    println!("  - Tick Spacing: {}", pool_state.tick_spacing);
                    
                    let converter = TickConverter { decimals_0: pool_state.mint_decimals_0, decimals_1: pool_state.mint_decimals_1 };
                    converter.print_all_prices(pool_state.tick_current);
                },
                RpcCommands::TokenMints { pool_id, rpc_url } => {
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let account_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    
                    let pool_state = PoolState::deserialize(&mut &account_data[8..]).expect("Failed to parse pool state");

                    println!("--- Token Mints for Pool {} ---", pool_id);
                    println!("  Token 0 (t0): {}", pool_state.token_mint_0);
                    println!("  Token 1 (t1): {}", pool_state.token_mint_1);
                },
                RpcCommands::DefaultBitmap { pool_id, rpc_url } => {
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let account_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &account_data[8..]).expect("Failed to parse pool state");
                    
                    println!("--- Initialized Tick Arrays (Default Bitmap) ---");
                    let helper = TickArrayHelper { tick_spacing: pool_state.tick_spacing };
                    let converter = TickConverter { decimals_0: pool_state.mint_decimals_0, decimals_1: pool_state.mint_decimals_1 };
                    let initialized = read_default_bitmap(&pool_state.tick_array_bitmap, pool_state.tick_spacing);
                    
                    println!("Found {} initialized arrays:", initialized.len());
                    for start_index in initialized {
                        println!("  - Start Index: {}", start_index);
                        let (tick_start, tick_end) = helper.get_array_tick_range(start_index);
                        
                        // T0 per T1
                        let p_start_t0_t1 = converter.tick_to_price(tick_start, PriceInput::Token0PerToken1Human{price: 0.0});
                        let p_end_t0_t1 = converter.tick_to_price(tick_end, PriceInput::Token0PerToken1Human{price: 0.0});
                        println!("      T0/T1 (Token0/Token1) Price Range: [{:.6}, {:.6}]", p_start_t0_t1, p_end_t0_t1);
                        
                        // T1 per T0
                        let p_start_t1_t0 = converter.tick_to_price(tick_start, PriceInput::Token1PerToken0Human{price: 0.0});
                        let p_end_t1_t0 = converter.tick_to_price(tick_end, PriceInput::Token1PerToken0Human{price: 0.0});
                        println!("      T1/T0 (Token1/Token0) Price Range: [{:.6}, {:.6}]", p_start_t1_t0, p_end_t1_t0);
                    }
                },
                RpcCommands::GetSwapArraysBlind { pool_id, direction, format, favorable_pct, impact_pct, price, rpc_url } => {
                    println!("--- Blind Swap Array Calculation for {} ---", pool_id);
                    println!("    (Assumes all arrays in range are initialized)");
                    
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();

                    // 1. Fetch ONLY PoolState (Needed for tick_spacing, decimals, current_tick)
                    println!("Fetching pool info...");
                    let pool_state_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &pool_state_data[8..]).expect("Failed to parse pool state");
                    println!("Done.");

                    // 2. Setup Helpers
                    let converter = TickConverter { decimals_0: pool_state.mint_decimals_0, decimals_1: pool_state.mint_decimals_1 };
                    let helper = TickArrayHelper { tick_spacing: pool_state.tick_spacing };

                    // 3. Determine Start Tick (Same as GetSwapArrays)
                    let tick_start = match price {
                        Some(p) => {
                            let price_input = match format {
                                HumanPriceFormat::T0PerT1 => PriceInput::Token0PerToken1Human { price: p },
                                HumanPriceFormat::T1PerT0 => PriceInput::Token1PerToken0Human { price: p },
                            };
                            println!("Using --price {:.8} as start", p);
                            converter.price_to_tick(price_input)
                        },
                        None => {
                            println!("No --price provided. Using live pool tick: {}", pool_state.tick_current);
                            pool_state.tick_current
                        }
                    };
                    
                    println!("Start Tick:    {}", tick_start);
                    println!("Direction:     {:?}", direction);
                    
                    // 4. Calculate Tick Range based on Direction (Same as GetSwapArrays)
                    let start_raw_price = converter.tick_to_raw_price(tick_start);
                    let (tick_favorable, tick_impact) = match direction {
                        SwapDirection::BuyT1 => { // Tick DECREASES
                            let favorable_raw_price = start_raw_price * (1.0 + (favorable_pct / 100.0));
                            let impact_raw_price = start_raw_price * (1.0 - (impact_pct / 100.0));
                            (converter.raw_price_to_tick(favorable_raw_price), converter.raw_price_to_tick(impact_raw_price))
                        },
                        SwapDirection::BuyT0 => { // Tick INCREASES
                            let favorable_raw_price = start_raw_price * (1.0 - (favorable_pct / 100.0));
                            let impact_raw_price = start_raw_price * (1.0 + (impact_pct / 100.0));
                            (converter.raw_price_to_tick(favorable_raw_price), converter.raw_price_to_tick(impact_raw_price))
                        }
                    };
                    let (min_tick, max_tick) = (tick_favorable.min(tick_impact), tick_favorable.max(tick_impact));
                    
                    println!("Favorable Pct: {:.4}%", favorable_pct);
                    println!("Impact Pct:    {:.4}%", impact_pct);
                    println!("Calculated Tick Range:  [{}, {}]", min_tick, max_tick);

                    // 5. Calculate Potential Arrays BLINDLY
                    let start_array_min = helper.get_array_start_index(min_tick);
                    let start_array_max = helper.get_array_start_index(max_tick);
                    let step = helper.tick_indices_per_array();

                    let mut potential_arrays = Vec::new();
                    let mut current_array_start = start_array_min;
                    while current_array_start <= start_array_max {
                        potential_arrays.push(current_array_start);
                        current_array_start += step;
                    }
                    
                    // 6. Define Core and Favorable Tick Ranges (Same as GetSwapArrays)
                    let (core_min_tick, core_max_tick) = (tick_start.min(tick_impact), tick_start.max(tick_impact));
                    let (favorable_min_tick, favorable_max_tick) = (tick_start.min(tick_favorable), tick_start.max(tick_favorable));

                    // 7. Split Potential Arrays into Core and Favorable (BLIND version)
                    let mut core_arrays: Vec<i32> = potential_arrays.iter()
                        .filter(|&&start_index| {
                            let (_tick_start, tick_end) = helper.get_array_tick_range(start_index);
                            (start_index <= core_max_tick) && (tick_end >= core_min_tick)
                        })
                        .cloned()
                        .collect();

                    let mut favorable_arrays: Vec<i32> = potential_arrays.iter()
                        .filter(|&&start_index| {
                            let (_tick_start, tick_end) = helper.get_array_tick_range(start_index);
                            let in_favorable_range = (start_index <= favorable_max_tick) && (tick_end >= favorable_min_tick);
                            let in_core_range = (start_index <= core_max_tick) && (tick_end >= core_min_tick);
                            in_favorable_range && !in_core_range
                        })
                        .cloned()
                        .collect();

                    // 8. Calculate the ONE surrounding array in the direction of IMPACT (BLIND version)
                    let surrounding_array: Option<(i32, &str)> = match direction { // <-- Assign directly from match
                        SwapDirection::BuyT1 => { // Impact is DOWN (tick decreases)
                            let surrounding_start_index = start_array_min - step;
                            Some((surrounding_start_index, "SURROUNDING_DN")) // <-- Return Some(...)
                        },
                        SwapDirection::BuyT0 => { // Impact is UP (tick increases)
                            let surrounding_start_index = start_array_max + step;
                            Some((surrounding_start_index, "SURROUNDING_UP")) // <-- Return Some(...)
                        },
                    };

                    // 9. Print Final List (Same as GetSwapArrays, uses the blind lists)
                    let total_arrays = core_arrays.len() + favorable_arrays.len() + if surrounding_array.is_some() { 1 } else { 0 };
                    println!("\n{:=<80}", "");
                    println!("--- REQUIRED SWAP ARRAYS (BLIND): {} ---", total_arrays);

                    match direction {
                        SwapDirection::BuyT1 => { // Tick DECREASES, print descending
                            favorable_arrays.sort_by(|a, b| b.cmp(a));
                            core_arrays.sort_by(|a, b| b.cmp(a));

                            for start_index in &favorable_arrays {
                                print_swap_array_info("FAVORABLE", *start_index, &pool_pubkey, &program_id);
                            }
                            if !favorable_arrays.is_empty() {
                                println!("\n{:-<80}", "");
                            }
                            for start_index in &core_arrays {
                                print_swap_array_info("CORE", *start_index, &pool_pubkey, &program_id);
                            }
                        },
                        SwapDirection::BuyT0 => { // Tick INCREASES, print ascending
                            // Arrays are already sorted ascending from the while loop
                            for start_index in &favorable_arrays {
                                print_swap_array_info("FAVORABLE", *start_index, &pool_pubkey, &program_id);
                            }
                            if !favorable_arrays.is_empty() {
                                println!("\n{:-<80}", "");
                            }
                            for start_index in &core_arrays {
                                print_swap_array_info("CORE", *start_index, &pool_pubkey, &program_id);
                            }
                        },
                    }

                    if let Some((start_index, label)) = surrounding_array {
                        if !core_arrays.is_empty() || !favorable_arrays.is_empty() {
                            println!("\n{:-<80}", "");
                        }
                        print_swap_array_info(label, start_index, &pool_pubkey, &program_id);
                    } else {
                        // This case is less likely in blind mode but kept for consistency
                        println!("\n[INFO] Surrounding array calculation resulted in an edge case (e.g., beyond max/min tick limits).");
                    }
                    println!("{:=<80}", "");

                },
                RpcCommands::GetSwapArrays { pool_id, direction, format, favorable_pct, impact_pct, price, rpc_url } => {
                    println!("--- Swap Array Calculation for {} ---", pool_id);
                    
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();

                    // 1. Fetch Base Data (PoolState + Extension)
                    println!("Fetching pool info and bitmaps...");
                    let pool_state_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &pool_state_data[8..]).expect("Failed to parse pool state");

                    let (ext_pda, _) = Pubkey::find_program_address(&[TICK_ARRAY_BITMAP_SEED, pool_pubkey.as_ref()], &program_id);
                    let ext_data = rpc_client.get_account_data(&ext_pda).expect("Failed to fetch bitmap extension");
                    let extension = TickArrayBitmapExtension::deserialize(&mut &ext_data[8..]).expect("Failed to parse bitmap extension");
                    println!("Done.");

                    // 2. Setup Helpers
                    let converter = TickConverter { decimals_0: pool_state.mint_decimals_0, decimals_1: pool_state.mint_decimals_1 };
                    let helper = TickArrayHelper { tick_spacing: pool_state.tick_spacing };

                    // 3. Determine Start Tick
                    let tick_start = match price {
                        Some(p) => {
                            let price_input = match format {
                                HumanPriceFormat::T0PerT1 => PriceInput::Token0PerToken1Human { price: p },
                                HumanPriceFormat::T1PerT0 => PriceInput::Token1PerToken0Human { price: p },
                            };
                            println!("Using --price {:.8} as start", p);
                            converter.price_to_tick(price_input)
                        },
                        None => {
                            println!("No --price provided. Using live pool tick: {}", pool_state.tick_current);
                            pool_state.tick_current
                        }
                    };
                    
                    println!("Start Tick:    {}", tick_start);
                    println!("Direction:     {:?}", direction);
                    
                    // 4. Calculate Tick Range based on Direction (using RAW PRICE)
                    //    (Based on on-chain facts: buy-t1 = tick decreases, buy-t0 = tick increases)
                    let start_raw_price = converter.tick_to_raw_price(tick_start);

                    let (tick_favorable, tick_impact) = match direction {
                        SwapDirection::BuyT1 => { // Tick DECREASES (Raw Price DECREASES)
                            // Favorable move = Tick INCREASES. To make tick INCREASE, `raw_price` must INCREASE.
                            let favorable_raw_price = start_raw_price * (1.0 + (favorable_pct / 100.0));
                            // Impact move = Tick DECREASES. To make tick DECREASE, `raw_price` must DECREASE.
                            let impact_raw_price = start_raw_price * (1.0 - (impact_pct / 100.0));
                            
                            (converter.raw_price_to_tick(favorable_raw_price), converter.raw_price_to_tick(impact_raw_price))
                        },
                        SwapDirection::BuyT0 => { // Tick INCREASES (Raw Price INCREASES)
                            // Favorable move = Tick DECREASES. To make tick DECREASE, `raw_price` must DECREASE.
                            let favorable_raw_price = start_raw_price * (1.0 - (favorable_pct / 100.0));
                            // Impact move = Tick INCREASES. To make tick INCREASE, `raw_price` must INCREASE.
                            let impact_raw_price = start_raw_price * (1.0 + (impact_pct / 100.0));
                            
                            (converter.raw_price_to_tick(favorable_raw_price), converter.raw_price_to_tick(impact_raw_price))
                        }
                    };

                    let (min_tick, max_tick) = (tick_favorable.min(tick_impact), tick_favorable.max(tick_impact));
                    
                    println!("Favorable Pct: {:.4}%", favorable_pct);
                    println!("Impact Pct:    {:.4}%", impact_pct);
                    println!("Calculated Tick Range:  [{}, {}]", min_tick, max_tick);

                    // 5. Get ALL initialized arrays and SORT them
                    let mut all_initialized_arrays = read_default_bitmap(&pool_state.tick_array_bitmap, pool_state.tick_spacing);
                    all_initialized_arrays.append(&mut read_extension_bitmap(&extension, pool_state.tick_spacing));
                    all_initialized_arrays.sort();

                    // 6. Filter and Find Arrays
                    let mut arrays_in_range: Vec<i32> = all_initialized_arrays.iter()
                        .filter(|&&start_index| {
                            let (_tick_start, tick_end) = helper.get_array_tick_range(start_index);
                            let array_start = start_index;
                            let array_end = tick_end; 
                            (array_start <= max_tick) && (array_end >= min_tick)
                        })
                        .cloned()
                        .collect();

                    // 7. Find the ONE surrounding array in the direction of IMPACT
                    let mut surrounding_array: Option<(i32, &str)> = None;
                    match direction {
                        SwapDirection::BuyT1 => { // Impact is DOWN (tick decreases)
                            if let Some(&start_index) = all_initialized_arrays.iter().filter(|&&s| helper.get_array_tick_range(s).1 < min_tick).last() {
                                surrounding_array = Some((start_index, "SURROUNDING_DN"));
                            }
                        },
                        SwapDirection::BuyT0 => { // Impact is UP (tick increases)
                            if let Some(&start_index) = all_initialized_arrays.iter().find(|&&s| s > max_tick) {
                                surrounding_array = Some((start_index, "SURROUNDING_UP"));
                            }
                        },
                    }

                    // 8. Print Final List in correct swap order
                    let total_arrays = arrays_in_range.len() + if surrounding_array.is_some() { 1 } else { 0 };
                    println!("\n{:=<80}", "");
                    println!("--- REQUIRED SWAP ARRAYS: {} ---", total_arrays);

                    match direction {
                        SwapDirection::BuyT1 => { // Tick DECREASES, so print in REVERSE (descending)
                            arrays_in_range.sort_by(|a, b| b.cmp(a)); // Sort descending
                            for start_index in &arrays_in_range {
                                print_swap_array_info("IN-RANGE", *start_index, &pool_pubkey, &program_id);
                            }
                        },
                        SwapDirection::BuyT0 => { // Tick INCREASES, so print in ORDER (ascending)
                            // .sort() was already called, so it's ascending
                            for start_index in &arrays_in_range {
                                print_swap_array_info("IN-RANGE", *start_index, &pool_pubkey, &program_id);
                            }
                        },
                    }

                    if let Some((start_index, label)) = surrounding_array {
                        print_swap_array_info(label, start_index, &pool_pubkey, &program_id);
                    } else {
                        println!("\n[WARNING] No initialized surrounding array found for the impact direction.");
                    }
                    println!("{:=<80}", "");

                },
                RpcCommands::ExtensionBitmap { pool_id, rpc_url } => {
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();
                    
                    // We need to fetch the main pool state to get decimals and tick_spacing
                    let pool_state_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &pool_state_data[8..]).expect("Failed to parse pool state");
                    
                    let (pda, _) = Pubkey::find_program_address(&[TICK_ARRAY_BITMAP_SEED, pool_pubkey.as_ref()], &program_id);
                    let account_data = rpc_client.get_account_data(&pda).expect("Failed to fetch bitmap extension");
                    let extension = TickArrayBitmapExtension::deserialize(&mut &account_data[8..]).expect("Failed to parse bitmap extension");

                    println!("--- Initialized Tick Arrays (Extension Bitmap) ---");
                    let helper = TickArrayHelper { tick_spacing: pool_state.tick_spacing };
                    let converter = TickConverter { decimals_0: pool_state.mint_decimals_0, decimals_1: pool_state.mint_decimals_1 };
                    let mut initialized = read_extension_bitmap(&extension, pool_state.tick_spacing);
                    initialized.sort(); // Sort for readability
                    
                    println!("Found {} initialized arrays in extension:", initialized.len());
                    for start_index in initialized {
                        println!("  - Start Index: {}", start_index);
                        let (tick_start, tick_end) = helper.get_array_tick_range(start_index);
                        
                        // T0 per T1
                        let p_start_t0_t1 = converter.tick_to_price(tick_start, PriceInput::Token0PerToken1Human{price: 0.0});
                        let p_end_t0_t1 = converter.tick_to_price(tick_end, PriceInput::Token0PerToken1Human{price: 0.0});
                        println!("      T0/T1 (Token0/Token1) Price Range: [{:.6}, {:.6}]", p_start_t0_t1, p_end_t0_t1);
                        
                        // T1 per T0
                        let p_start_t1_t0 = converter.tick_to_price(tick_start, PriceInput::Token1PerToken0Human{price: 0.0});
                        let p_end_t1_t0 = converter.tick_to_price(tick_end, PriceInput::Token1PerToken0Human{price: 0.0});
                        println!("      T1/T0 (Token1/Token0) Price Range: [{:.6}, {:.6}]", p_start_t1_t0, p_end_t1_t0);
                    }
                },
                RpcCommands::TickArray { pool_id, start_index, rpc_url } => {
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();

                    // First, fetch pool state to get decimals and tick_spacing
                    let pool_account_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &pool_account_data[8..]).expect("Failed to parse pool state");
                    
                    let helper = TickArrayHelper { tick_spacing: pool_state.tick_spacing };
                    let converter = TickConverter { decimals_0: pool_state.mint_decimals_0, decimals_1: pool_state.mint_decimals_1 };
                    
                    // Now, fetch the tick array
                    let (pda, _) = Pubkey::find_program_address(&[TICK_ARRAY_SEED, pool_pubkey.as_ref(), &start_index.to_be_bytes()], &program_id);
                    let account_data = rpc_client.get_account_data(&pda).expect("Failed to fetch tick array");
                    let tick_array = TickArrayState::deserialize(&mut &account_data[8..]).expect("Failed to parse tick array");
                    
                    println!("--- Tick Array Details (Start Index: {}) ---", tick_array.start_tick_index);
                    
                    // Print Price Range for the entire array
                    let (tick_start, tick_end) = helper.get_array_tick_range(tick_array.start_tick_index);
                    println!("\nPrice Range for this Array (Tick {} to {}):", tick_start, tick_end);

                    // T0 per T1
                    let p_start_t0_t1 = converter.tick_to_price(tick_start, PriceInput::Token0PerToken1Human{price: 0.0});
                    let p_end_t0_t1 = converter.tick_to_price(tick_end, PriceInput::Token0PerToken1Human{price: 0.0});
                    println!("  - T0/T1 (Token0/Token1): [{:.6}, {:.6}]", p_start_t0_t1, p_end_t0_t1);

                    // T1 per T0
                    let p_start_t1_t0 = converter.tick_to_price(tick_start, PriceInput::Token1PerToken0Human{price: 0.0});
                    let p_end_t1_t0 = converter.tick_to_price(tick_end, PriceInput::Token1PerToken0Human{price: 0.0});
                    println!("  - T1/T0 (Token1/Token0): [{:.6}, {:.6}]", p_start_t1_t0, p_end_t1_t0);
                    
                    println!("\n  - Initialized Ticks: {}", tick_array.initialized_tick_count);
                    for tick_state in tick_array.ticks.iter() {
                        if tick_state.liquidity_gross != 0 {
                            println!("    - Tick {}:", tick_state.tick);
                            println!("        LiquidityNet:  {}", tick_state.liquidity_net);
                            println!("        LiquidityGross: {}", tick_state.liquidity_gross);
                        }
                    }
                },
                RpcCommands::InitializedRangePercent { pool_id, price, lower_pct, upper_pct, format, rpc_url } => {
                    // Calculate the price range from percentages
                    let price_lower = price * (1.0 - (lower_pct / 100.0));
                    let price_upper = price * (1.0 + (upper_pct / 100.0));

                    println!("--- Initialized Array Percent Range Analysis for {} ---", pool_id);
                    println!("Base Price:   {:.8}", price);
                    println!("Range:        -{:.2}% to +{:.2}%", lower_pct, upper_pct);
                    println!("Calculated Price Range: [{:.8}, {:.8}]", price_lower, price_upper);
                    
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();

                    // 1. Fetch Base Data (PoolState + Extension)
                    println!("Fetching pool info and bitmaps...");
                    let pool_state_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &pool_state_data[8..]).expect("Failed to parse pool state");

                    let (ext_pda, _) = Pubkey::find_program_address(&[TICK_ARRAY_BITMAP_SEED, pool_pubkey.as_ref()], &program_id);
                    let ext_data = rpc_client.get_account_data(&ext_pda).expect("Failed to fetch bitmap extension");
                    let extension = TickArrayBitmapExtension::deserialize(&mut &ext_data[8..]).expect("Failed to parse bitmap extension");
                    println!("Done.");

                    // 2. Setup Helpers
                    let converter = TickConverter { decimals_0: pool_state.mint_decimals_0, decimals_1: pool_state.mint_decimals_1 };
                    let helper = TickArrayHelper { tick_spacing: pool_state.tick_spacing };
                    
                    // 3. Convert Price Range to Tick Range
                    let (price_format_lower, price_format_upper, price_template) = match format {
                        HumanPriceFormat::T0PerT1 => (
                            PriceInput::Token0PerToken1Human { price: price_lower },
                            PriceInput::Token0PerToken1Human { price: price_upper },
                            PriceInput::Token0PerToken1Human { price: 0.0 }, // Template for printing
                        ),
                        HumanPriceFormat::T1PerT0 => (
                            PriceInput::Token1PerToken0Human { price: price_lower },
                            PriceInput::Token1PerToken0Human { price: price_upper },
                            PriceInput::Token1PerToken0Human { price: 0.0 }, // Template for printing
                        ),
                    };

                    let tick_lower = converter.price_to_tick(price_format_lower);
                    let tick_upper = converter.price_to_tick(price_format_upper);
                    
                    // Ensure min_tick is always the smaller number, max_tick is larger
                    let (min_tick, max_tick) = if tick_lower > tick_upper {
                        (tick_upper, tick_lower)
                    } else {
                        (tick_lower, tick_upper)
                    };

                    println!("Input Price Range [{:.8}, {:.8}] maps to Tick Range [{}, {}]", price_lower, price_upper, min_tick, max_tick);

                    // 4. Get ALL initialized arrays and SORT them
                    let mut all_initialized_arrays = read_default_bitmap(&pool_state.tick_array_bitmap, pool_state.tick_spacing);
                    all_initialized_arrays.append(&mut read_extension_bitmap(&extension, pool_state.tick_spacing));
                    all_initialized_arrays.sort();

                    // 5. Filter and Find Arrays
                    let arrays_in_range: Vec<i32> = all_initialized_arrays.iter()
                        .filter(|&&start_index| {
                            let (_tick_start, tick_end) = helper.get_array_tick_range(start_index);
                            let array_start = start_index;
                            let array_end = tick_end; 
                            (array_start <= max_tick) && (array_end >= min_tick)
                        })
                        .cloned()
                        .collect();

                    let lower_surrounding: Option<i32> = all_initialized_arrays.iter()
                        .filter(|&&start_index| {
                            let (_tick_start, tick_end) = helper.get_array_tick_range(start_index);
                            tick_end < min_tick 
                        })
                        .last() 
                        .cloned();

                    let upper_surrounding: Option<i32> = all_initialized_arrays.iter()
                        .filter(|&&start_index| {
                            start_index > max_tick 
                        })
                        .next() 
                        .cloned();

                    // 6. Fetch and Print Details
                    if let Some(start_index) = lower_surrounding {
                        println!("\n{:-<80}", "");
                        println!("--- (Lower Surrounding Initialized Array) ---");
                        fetch_and_print_array_details(&rpc_client, &pool_pubkey, &program_id, start_index, &converter, &helper, price_template);
                    } else {
                        println!("\n{:-<80}", "");
                        println!("--- (No initialized array found below price range) ---");
                    }

                    println!("\n{:=<80}", "");
                    println!("--- ARRAYS INITIALIZED WITHIN PRICE RANGE ({}) ---", arrays_in_range.len());
                    if arrays_in_range.is_empty() {
                        println!("--- (No initialized arrays found within price range) ---");
                    } else {
                        for start_index in arrays_in_range {
                            fetch_and_print_array_details(&rpc_client, &pool_pubkey, &program_id, start_index, &converter, &helper, price_template);
                        }
                    }
                    println!("{:=<80}", "");


                    if let Some(start_index) = upper_surrounding {
                        println!("\n{:-<80}", "");
                        println!("--- (Upper Surrounding Initialized Array) ---");
                        fetch_and_print_array_details(&rpc_client, &pool_pubkey, &program_id, start_index, &converter, &helper, price_template);
                    } else {
                        println!("\n{:-<80}", "");
                        println!("--- (No initialized array found above price range) ---");
                    }
                },
                RpcCommands::InitializedRange { pool_id, price_lower, price_upper, format, rpc_url } => {
                    println!("--- Initialized Array Range Analysis for {} ---", pool_id);
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();

                    // 1. Fetch Base Data (PoolState + Extension)
                    println!("Fetching pool info and bitmaps...");
                    let pool_state_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &pool_state_data[8..]).expect("Failed to parse pool state");

                    let (ext_pda, _) = Pubkey::find_program_address(&[TICK_ARRAY_BITMAP_SEED, pool_pubkey.as_ref()], &program_id);
                    let ext_data = rpc_client.get_account_data(&ext_pda).expect("Failed to fetch bitmap extension");
                    let extension = TickArrayBitmapExtension::deserialize(&mut &ext_data[8..]).expect("Failed to parse bitmap extension");
                    println!("Done.");

                    // 2. Setup Helpers
                    let converter = TickConverter { decimals_0: pool_state.mint_decimals_0, decimals_1: pool_state.mint_decimals_1 };
                    let helper = TickArrayHelper { tick_spacing: pool_state.tick_spacing };
                    
                    // 3. Convert Price Range to Tick Range
                    let (price_format_lower, price_format_upper, price_template) = match format {
                        HumanPriceFormat::T0PerT1 => (
                            PriceInput::Token0PerToken1Human { price: price_lower },
                            PriceInput::Token0PerToken1Human { price: price_upper },
                            PriceInput::Token0PerToken1Human { price: 0.0 }, // Template for printing
                        ),
                        HumanPriceFormat::T1PerT0 => (
                            PriceInput::Token1PerToken0Human { price: price_lower },
                            PriceInput::Token1PerToken0Human { price: price_upper },
                            PriceInput::Token1PerToken0Human { price: 0.0 }, // Template for printing
                        ),
                    };

                    let tick_lower = converter.price_to_tick(price_format_lower);
                    let tick_upper = converter.price_to_tick(price_format_upper);
                    
                    // Ensure min_tick is always the smaller number, max_tick is larger
                    let (min_tick, max_tick) = if tick_lower > tick_upper {
                        (tick_upper, tick_lower)
                    } else {
                        (tick_lower, tick_upper)
                    };

                    println!("Input Price Range [{:.6}, {:.6}] maps to Tick Range [{}, {}]", price_lower, price_upper, min_tick, max_tick);

                    // 4. Get ALL initialized arrays and SORT them
                    let mut all_initialized_arrays = read_default_bitmap(&pool_state.tick_array_bitmap, pool_state.tick_spacing);
                    all_initialized_arrays.append(&mut read_extension_bitmap(&extension, pool_state.tick_spacing));
                    all_initialized_arrays.sort();

                    // 5. Filter and Find Arrays
                    let arrays_in_range: Vec<i32> = all_initialized_arrays.iter()
                        .filter(|&&start_index| {
                            let (_tick_start, tick_end) = helper.get_array_tick_range(start_index);
                            // An array overlaps the range if:
                            // (array_start <= max_tick) AND (array_end >= min_tick)
                            let array_start = start_index;
                            let array_end = tick_end; // tick_end from helper is inclusive
                            (array_start <= max_tick) && (array_end >= min_tick)
                        })
                        .cloned()
                        .collect();

                    let lower_surrounding: Option<i32> = all_initialized_arrays.iter()
                        .filter(|&&start_index| {
                            let (_tick_start, tick_end) = helper.get_array_tick_range(start_index);
                            tick_end < min_tick // Find arrays that *end* before our range starts
                        })
                        .last() // Get the one closest (last) to the range
                        .cloned();

                    let upper_surrounding: Option<i32> = all_initialized_arrays.iter()
                        .filter(|&&start_index| {
                            start_index > max_tick // Find arrays that *start* after our range ends
                        })
                        .next() // Get the one closest (first) to the range
                        .cloned();

                    // 6. Fetch and Print Details using the new helper function

                    if let Some(start_index) = lower_surrounding {
                        println!("\n{:-<80}", "");
                        println!("--- (Lower Surrounding Initialized Array) ---");
                        fetch_and_print_array_details(&rpc_client, &pool_pubkey, &program_id, start_index, &converter, &helper, price_template);
                    } else {
                        println!("\n{:-<80}", "");
                        println!("--- (No initialized array found below price range) ---");
                    }

                    println!("\n{:=<80}", "");
                    println!("--- ARRAYS INITIALIZED WITHIN PRICE RANGE ({}) ---", arrays_in_range.len());
                    if arrays_in_range.is_empty() {
                        println!("--- (No initialized arrays found within price range) ---");
                    } else {
                        for start_index in arrays_in_range {
                            fetch_and_print_array_details(&rpc_client, &pool_pubkey, &program_id, start_index, &converter, &helper, price_template);
                        }
                    }
                    println!("{:=<80}", "");


                    if let Some(start_index) = upper_surrounding {
                        println!("\n{:-<80}", "");
                        println!("--- (Upper Surrounding Initialized Array) ---");
                        fetch_and_print_array_details(&rpc_client, &pool_pubkey, &program_id, start_index, &converter, &helper, price_template);
                    } else {
                        println!("\n{:-<80}", "");
                        println!("--- (No initialized array found above price range) ---");
                    }
                },
                RpcCommands::LiquidityCurve { pool_id, format, max_width, rpc_url, show_arrays } => {
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();

                    println!("Fetching pool info and bitmaps...");
                    let pool_state_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &pool_state_data[8..]).expect("Failed to parse pool state");
                    
                    let (ext_pda, _) = Pubkey::find_program_address(&[TICK_ARRAY_BITMAP_SEED, pool_pubkey.as_ref()], &program_id);
                    let ext_data = rpc_client.get_account_data(&ext_pda).expect("Failed to fetch bitmap extension");
                    let extension = TickArrayBitmapExtension::deserialize(&mut &ext_data[8..]).expect("Failed to parse bitmap extension");

                    // Get all initialized array start indices from bitmaps
                    let mut all_initialized_arrays = read_default_bitmap(&pool_state.tick_array_bitmap, pool_state.tick_spacing);
                    all_initialized_arrays.append(&mut read_extension_bitmap(&extension, pool_state.tick_spacing));

                    println!(
                        "Found {} initialized tick arrays. Fetching each account... (this will be slow)",
                        all_initialized_arrays.len()
                    );

                    // Fetch each tick array individually and extract ticks
                    let mut all_ticks = Vec::new();
                    for start_index in all_initialized_arrays {
                        let (pda, _) = Pubkey::find_program_address(&[TICK_ARRAY_SEED, pool_pubkey.as_ref(), &start_index.to_be_bytes()], &program_id);
                        if let Ok(account_data) = rpc_client.get_account_data(&pda) {
                            if let Ok(tick_array) = TickArrayState::deserialize(&mut &account_data[8..]) {
                                for tick_state in tick_array.ticks.iter() {
                                    if tick_state.liquidity_gross != 0 {
                                        all_ticks.push((tick_state.tick, tick_state.liquidity_net));
                                    }
                                }
                            }
                        }
                    }
                    
                    println!("Done fetching and parsing.");
                    
                    let converter = TickConverter {
                        decimals_0: pool_state.mint_decimals_0,
                        decimals_1: pool_state.mint_decimals_1,
                    };

                    let price_format_template = match format {
                        HumanPriceFormat::T0PerT1 => PriceInput::Token0PerToken1Human { price: 0.0 },
                        HumanPriceFormat::T1PerT0 => PriceInput::Token1PerToken0Human { price: 0.0 },
                    };

                    print_exact_liquidity_ranges(
                        &mut all_ticks,
                        &converter,
                        price_format_template,
                        max_width,
                        pool_state.tick_current,
                        pool_state.tick_spacing,
                        &pool_pubkey,
                        &program_id,
                        show_arrays,
                    );

                },
                RpcCommands::InspectArray { pool_id, start_index, pda, rpc_url } => {
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();

                    // Determine the PDA from the provided input (either start_index or pda)
                    let tick_array_pda = if let Some(start_idx) = start_index {
                        println!("Deriving PDA from start index {}...", start_idx);
                        Pubkey::find_program_address(&[TICK_ARRAY_SEED, pool_pubkey.as_ref(), &start_idx.to_be_bytes()], &program_id).0
                    } else if let Some(pda_str) = pda {
                        println!("Using provided PDA {}...", &pda_str);
                        Pubkey::from_str(&pda_str).expect("Invalid PDA format")
                    } else {
                        // This case is prevented by clap's `group` attribute, but we handle it anyway
                        eprintln!("Error: You must provide either --start-index or --pda.");
                        return;
                    };

                    println!("Fetching account data for PDA: {}", tick_array_pda);
                    let account_data = rpc_client.get_account_data(&tick_array_pda).expect("Failed to fetch tick array");
                    let tick_array = TickArrayState::deserialize(&mut &account_data[8..]).expect("Failed to parse tick array");
                    
                    // We still need tick_spacing from the main pool state for correct visualization
                    let pool_state_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &pool_state_data[8..]).expect("Failed to parse pool state");
                    
                    println!("Done.");

                    // Call the visualization function, now passing the PDA to be printed
                    print_tick_array_visualization(&tick_array, pool_state.tick_spacing, &tick_array_pda);
                },
                RpcCommands::FullAnalysis { pool_id, format, rpc_url } => {
                    let rpc_client = RpcClient::new(rpc_url);
                    let pool_pubkey = Pubkey::from_str(&pool_id).expect("Invalid Pool ID");
                    let program_id = Pubkey::from_str(RAYDIUM_CLMM_PROGRAM_ID).unwrap();

                    // 1. Fetch both Pool State and Extension Bitmap
                    println!("Fetching on-chain data...");
                    let pool_state_data = rpc_client.get_account_data(&pool_pubkey).expect("Failed to fetch pool state");
                    let pool_state = PoolState::deserialize(&mut &pool_state_data[8..]).expect("Failed to parse pool state");

                    let (ext_pda, _) = Pubkey::find_program_address(&[TICK_ARRAY_BITMAP_SEED, pool_pubkey.as_ref()], &program_id);
                    let ext_data = rpc_client.get_account_data(&ext_pda).expect("Failed to fetch bitmap extension");
                    let extension = TickArrayBitmapExtension::deserialize(&mut &ext_data[8..]).expect("Failed to parse bitmap extension");
                    println!("Done.");

                    // 2. Setup helpers
                    let helper = TickArrayHelper { tick_spacing: pool_state.tick_spacing };
                    let converter = TickConverter { decimals_0: pool_state.mint_decimals_0, decimals_1: pool_state.mint_decimals_1 };
                    
                    // 3. Combine and sort all initialized arrays
                    let mut initialized_default = read_default_bitmap(&pool_state.tick_array_bitmap, pool_state.tick_spacing);
                    let mut initialized_extension = read_extension_bitmap(&extension, pool_state.tick_spacing);
                    initialized_default.append(&mut initialized_extension);
                    initialized_default.sort();
                    let all_initialized_arrays = initialized_default;

                    // 4. Find the current array
                    // let current_array_start_index = helper.get_array_start_index(pool_state.tick_current);

                    // 5. Determine user's desired price format
                    let (price_template, format_label) = match format {
                        HumanPriceFormat::T0PerT1 => (PriceInput::Token0PerToken1Human{price: 0.0}, "T0/T1 (Token0/Token1)"),
                        HumanPriceFormat::T1PerT0 => (PriceInput::Token1PerToken0Human{price: 0.0}, "T1/T0 (Token1/Token0)"),
                    };

                    println!("\n--- Full Liquidity Analysis for {} ---", pool_id);
                    println!("Current Tick: {}", pool_state.tick_current);

                    println!("\n{:<15} | {}", "Array Start/Tick", "Price / Price Range");
                    println!("{:-<75}", "");

                    let mut current_tick_printed = false;

                    for &start_index in &all_initialized_arrays {
                        // Check if the current tick's position is BEFORE the next array to be printed.
                        if !current_tick_printed && start_index > pool_state.tick_current {
                            let current_price = converter.tick_to_price(pool_state.tick_current, price_template);
                            println!("{:-<75}", "");
                            println!(
                                "{:<15} | Price: {:.6}               <-- YOU ARE HERE",
                                format!("Tick {}", pool_state.tick_current),
                                current_price
                            );
                            println!("{:-<75}", "");
                            current_tick_printed = true;
                        }

                        // Print the array's info
                        let (tick_start, tick_end) = helper.get_array_tick_range(start_index);
                        let price_start = converter.tick_to_price(tick_start, price_template);
                        let price_end = converter.tick_to_price(tick_end, price_template);
                        
                        println!(
                            "{:<15} | [{:.6}, {:.6}]",
                            start_index,
                            price_start,
                            price_end,
                        );
                    }

                    // This handles the case where the current tick is after the last initialized array in the list.
                    if !current_tick_printed {
                        let current_price = converter.tick_to_price(pool_state.tick_current, price_template);
                        println!("{:-<75}", "");
                        println!(
                            "{:<15} | Price: {:.6}               <-- YOU ARE HERE",
                            format!("Tick {}", pool_state.tick_current),
                            current_price
                        );
                        println!("{:-<75}", "");
                    }
                    println!("\nPrice format is: {}", format_label);
                },
            }
        }
    }
}

// --- New Bitmap Reader Functions ---

/// Reads the default 1024-bit bitmap from the PoolState.
fn read_default_bitmap(bitmap: &[u64; 16], tick_spacing: u16) -> Vec<i32> {
    let mut initialized = Vec::new();
    let ticks_per_array = TICK_ARRAY_SIZE * tick_spacing as i32;

    for (word_idx, &word) in bitmap.iter().enumerate() {
        if word == 0 { continue; }
        for bit_idx in 0..64 {
            if (word & (1u64 << bit_idx)) != 0 {
                let global_bit_pos = (word_idx * 64 + bit_idx) as i32;
                // Default bitmap is centered. 512 is the center offset.
                let array_offset = global_bit_pos - 512;
                let start_index = array_offset * ticks_per_array;
                initialized.push(start_index);
            }
        }
    }
    initialized
}

/// Prints the array start index and PDA for the swap-arrays command.
fn print_swap_array_info(
    label: &str,
    start_index: i32,
    pool_pubkey: &Pubkey,
    program_id: &Pubkey,
) {
    let (pda, _bump) = Pubkey::find_program_address(
        &[
            TICK_ARRAY_SEED,
            pool_pubkey.as_ref(),
            &start_index.to_be_bytes(),
        ],
        program_id,
    );
    println!("\n[{:^13}] Array Start Index: {}", label, start_index);
    println!("                  PDA: {}", pda);
}

/// Fetches, parses, and prints a detailed breakdown of a single Tick Array.
fn fetch_and_print_array_details(
    rpc_client: &RpcClient,
    pool_pubkey: &Pubkey,
    program_id: &Pubkey,
    start_index: i32,
    converter: &TickConverter,
    helper: &TickArrayHelper,
    price_template: PriceInput, // To print price ranges in the user's format
) {
    // 1. Derive PDA
    let (pda, _bump) = Pubkey::find_program_address(
        &[
            TICK_ARRAY_SEED,
            pool_pubkey.as_ref(),
            &start_index.to_be_bytes(),
        ],
        program_id,
    );

    println!("\n--- Array Start Index: {} ---", start_index);
    println!("  PDA Address: {}", pda);

    // 2. Print Price Range
    let (tick_start, tick_end) = helper.get_array_tick_range(start_index);
    let price_start = converter.tick_to_price(tick_start, price_template);
    let price_end = converter.tick_to_price(tick_end, price_template);
    // Handle price inversion for readability
    let (p_start, p_end) = if price_start < price_end { (price_start, price_end) } else { (price_end, price_start) };
    println!("  Price Range: [{:.8}, {:.8}]", p_start, p_end);
    println!("  Tick Range:  [{}, {}]", tick_start, tick_end);


    // 3. Fetch and Parse
    match rpc_client.get_account_data(&pda) {
        Ok(account_data) => {
            match TickArrayState::deserialize(&mut &account_data[8..]) {
                Ok(tick_array) => {
                    println!("  Initialized Ticks: {}/{}", tick_array.initialized_tick_count, TICK_ARRAY_SIZE);
                    
                    if tick_array.initialized_tick_count == 0 {
                        println!("  (Array is initialized but contains no active ticks)");
                        return;
                    }

                    // 4. Print detailed tick info
                    println!("  --- Initialized Tick Details ---");
                    for (slot_index, tick_state) in tick_array.ticks.iter().enumerate() {
                        if tick_state.liquidity_gross != 0 {
                            // This is an initialized tick
                            println!("    - Slot (Modulo) {}:", slot_index);
                            println!("        Raw Tick Index: {}", tick_state.tick);
                            println!("        Liquidity Net:  {}", tick_state.liquidity_net);
                            println!("        Liquidity Gross:{}", tick_state.liquidity_gross);
                        }
                    }
                },
                Err(e) => {
                    println!("  ERROR: Failed to parse TickArrayState for PDA {}: {}", pda, e);
                }
            }
        },
        Err(e) => {
            println!("  ERROR: Failed to fetch account data for PDA {}: {}", pda, e);
        }
    }
}

/// Reads the extension bitmap.
fn read_extension_bitmap(extension: &TickArrayBitmapExtension, tick_spacing: u16) -> Vec<i32> {
    let mut initialized = Vec::new();
    let ticks_per_array = TICK_ARRAY_SIZE * tick_spacing as i32;
    let arrays_per_bitmap = 512;

    // Positive Bitmaps
    for (bitmap_idx, bitmap_chunk) in extension.positive_tick_array_bitmap.iter().enumerate() {
        for (word_idx, &word) in bitmap_chunk.iter().enumerate() {
            if word == 0 { continue; }
            for bit_idx in 0..64 {
                if (word & (1u64 << bit_idx)) != 0 {
                    let bit_pos_in_chunk = (word_idx * 64 + bit_idx) as i32;
                    // The default bitmap has 512 positive slots.
                    // Extension 0 starts after that, at array offset 512.
                    let array_offset = 512 + (bitmap_idx as i32 * arrays_per_bitmap) + bit_pos_in_chunk;
                    let start_index = array_offset * ticks_per_array;
                    initialized.push(start_index);
                }
            }
        }
    }
    
    // Negative Bitmaps
    for (bitmap_idx, bitmap_chunk) in extension.negative_tick_array_bitmap.iter().enumerate() {  
        for (word_idx, &word) in bitmap_chunk.iter().enumerate() {  
            if word == 0 { continue; }  
            for bit_idx in 0..64 {  
                if (word & (1u64 << bit_idx)) != 0 {  
                    let bit_pos_in_chunk = (word_idx * 64 + bit_idx) as i32;  
                    // For negative arrays, the bit position maps differently  
                    // The bitmap is reversed for negative indices  
                    let offset_in_bitmap = 511 - bit_pos_in_chunk;  
                    let array_offset = -513 - (bitmap_idx as i32 * arrays_per_bitmap) - offset_in_bitmap;  
                    let start_index = array_offset * ticks_per_array;  
                    initialized.push(start_index);  
                }  
            }  
        }  
    }

    initialized
}
