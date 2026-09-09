pub mod constants;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("C8Y9h7TEePGLvZK9YJBgRxAVdwEpVtv1r5duvXMtK7xt");

#[program]
pub mod escrow {
    use super::*;

    // Maker opens a trade: locks `deposit` of mint_a and asks for `receive` of mint_b.
    pub fn make(ctx: Context<Make>, seed: u64, deposit: u64, receive: u64) -> Result<()> {
        handle_make(ctx, seed, deposit, receive)
    }

    // Taker fills the trade: pays the maker in mint_b, walks away with mint_a.
    pub fn take(ctx: Context<Take>) -> Result<()> {
        handle_take(ctx)
    }

    // Maker changes their mind about the price while the trade is still open.
    pub fn update(ctx: Context<Update>, receive: u64) -> Result<()> {
        handle_update(ctx, receive)
    }

    // Maker calls off the trade and takes their mint_a back.
    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        handle_refund(ctx)
    }
}
