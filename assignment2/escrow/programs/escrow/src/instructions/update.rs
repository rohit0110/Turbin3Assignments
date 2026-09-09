use anchor_lang::prelude::*;

use crate::{constants::*, state::Escrow};

#[derive(Accounts)]
pub struct Update<'info> {
    #[account(mut)]
    pub maker: Signer<'info>,

    // Only the maker can touch their own escrow, hence `has_one = maker`.
    #[account(
        mut,
        has_one = maker,
        seeds = [ESCROW_SEED, maker.key().as_ref(), &escrow.seed.to_le_bytes()],
        bump = escrow.bump,
    )]
    pub escrow: Account<'info, Escrow>,
}

pub fn handle_update(ctx: Context<Update>, receive: u64) -> Result<()> {
    // Just re-price the trade. Tokens already in the vault stay put.
    ctx.accounts.escrow.receive = receive;
    Ok(())
}
