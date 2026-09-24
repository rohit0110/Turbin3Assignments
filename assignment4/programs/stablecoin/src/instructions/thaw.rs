use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use spl_token_2022_interface::instruction::thaw_account;

// Task 4: KYC unfreeze of one account. Does not touch the mint's
// DefaultAccountState, which would change the freeze policy for future accounts.
#[derive(Accounts)]
pub struct ThawAccount<'info> {
    pub freeze_authority: Signer<'info>,

    #[account(mut)]
    /// CHECK: ownership/mint-linkage validated by the token-2022 CPI itself.
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: only read for its pubkey.
    pub mint: UncheckedAccount<'info>,

    /// CHECK: address-constrained to the real Token-2022 program.
    #[account(address = spl_token_2022_interface::id())]
    pub token_program: UncheckedAccount<'info>,
}

pub fn handle_thaw_account(ctx: Context<ThawAccount>) -> Result<()> {
    let token_program_id = spl_token_2022_interface::id();
    let ix = thaw_account(
        &token_program_id,
        ctx.accounts.token_account.key,
        ctx.accounts.mint.key,
        ctx.accounts.freeze_authority.key,
        &[],
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.freeze_authority.to_account_info(),
        ],
    )?;

    Ok(())
}
