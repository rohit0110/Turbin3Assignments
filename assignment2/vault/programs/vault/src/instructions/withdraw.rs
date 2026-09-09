use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

use crate::{constants::*, error::VaultError, state::VaultState};

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        seeds = [VAULT_STATE_SEED, user.key().as_ref()],
        bump = vault_state.state_bump,
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        seeds = [VAULT_SEED, vault_state.key().as_ref()],
        bump = vault_state.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
    // Bail out early with a clear error instead of letting the system
    // program fail the transfer with a generic one.
    require!(
        ctx.accounts.vault.lamports() >= amount,
        VaultError::InsufficientBalance
    );

    // The vault is a PDA, so the program has to sign on its behalf.
    let vault_state_key = ctx.accounts.vault_state.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        VAULT_SEED,
        vault_state_key.as_ref(),
        &[ctx.accounts.vault_state.vault_bump],
    ]];

    let cpi_ctx = CpiContext::new_with_signer(
        ctx.accounts.system_program.key(),
        Transfer {
            from: ctx.accounts.vault.to_account_info(),
            to: ctx.accounts.user.to_account_info(),
        },
        signer_seeds,
    );
    transfer(cpi_ctx, amount)
}
