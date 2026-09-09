use anchor_lang::prelude::*;

use crate::{constants::*, state::VaultState};

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    // The state account is a PDA off the user's key, so there is exactly
    // one vault per user and nobody has to pass an address around.
    #[account(
        init,
        payer = user,
        space = 8 + VaultState::INIT_SPACE,
        seeds = [VAULT_STATE_SEED, user.key().as_ref()],
        bump,
    )]
    pub vault_state: Account<'info, VaultState>,

    // The vault itself: no data, just a system-owned account that holds SOL.
    // Nothing to init here, a transfer will bring it to life on first deposit.
    #[account(
        seeds = [VAULT_SEED, vault_state.key().as_ref()],
        bump,
    )]
    pub vault: SystemAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize(ctx: Context<Initialize>) -> Result<()> {
    // Remember the bumps
    ctx.accounts.vault_state.vault_bump = ctx.bumps.vault;
    ctx.accounts.vault_state.state_bump = ctx.bumps.vault_state;

    //No need to add extra CPI instruction to transfer Rent Exempt amt since deposit will be creating it anyway
    Ok(())
}
