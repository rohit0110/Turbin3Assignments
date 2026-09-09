pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("CABoyqGaLbnjpiomqmsRtryvx7v2Tf1oQA8Y1mUTTnKX");

#[program]
pub mod vault {
    use super::*;

    // One-time setup: create the state account and record the bumps.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        handle_initialize(ctx)
    }

    // Move `amount` lamports from the user into their vault PDA.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        handle_deposit(ctx, amount)
    }

    // Pull `amount` lamports back out of the vault to the user.
    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        handle_withdraw(ctx, amount)
    }

    // Drain whatever is left and close the state account.
    pub fn close(ctx: Context<Close>) -> Result<()> {
        handle_close(ctx)
    }
}
