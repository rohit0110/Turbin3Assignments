use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use spl_token_2022_interface::extension::confidential_transfer::instruction::deposit;

// Task 6, step 2: DepositConfidentialTokens. Moves amount from the public
// balance into the pending confidential balance. No proof needed -- the
// amount is plaintext on both sides.
#[derive(Accounts)]
pub struct DepositConfidential<'info> {
    pub authority: Signer<'info>,

    #[account(mut)]
    /// CHECK: ownership validated by the token-2022 CPI itself.
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: only read for its pubkey.
    pub mint: UncheckedAccount<'info>,

    /// CHECK: address-constrained to the real Token-2022 program.
    #[account(address = spl_token_2022_interface::id())]
    pub token_program: UncheckedAccount<'info>,
}

pub fn handle_deposit_confidential(
    ctx: Context<DepositConfidential>,
    amount: u64,
    decimals: u8,
) -> Result<()> {
    let token_program_id = spl_token_2022_interface::id();

    let ix = deposit(
        &token_program_id,
        ctx.accounts.token_account.key,
        ctx.accounts.mint.key,
        amount,
        decimals,
        ctx.accounts.authority.key,
        &[],
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.authority.to_account_info(),
        ],
    )?;

    Ok(())
}
