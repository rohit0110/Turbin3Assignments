use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use spl_token_2022_interface::extension::confidential_transfer::instruction::inner_apply_pending_balance;
use spl_token_2022_interface::extension::confidential_transfer::DecryptableBalance;

// Task 6, step 3: ApplyPendingBalance. Folds the pending balance into the
// available balance. No proof needed -- the owner supplies the re-encrypted
// balance themselves; expected_pending_balance_credit_counter guards against
// a credit landing mid-flight.
#[derive(Accounts)]
pub struct ApplyPendingBalance<'info> {
    pub owner: Signer<'info>,

    #[account(mut)]
    /// CHECK: ownership validated by the token-2022 CPI itself.
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: address-constrained to the real Token-2022 program.
    #[account(address = spl_token_2022_interface::id())]
    pub token_program: UncheckedAccount<'info>,
}

pub fn handle_apply_pending_balance(
    ctx: Context<ApplyPendingBalance>,
    expected_pending_balance_credit_counter: u64,
    new_decryptable_available_balance: [u8; 36],
) -> Result<()> {
    let token_program_id = spl_token_2022_interface::id();
    let new_balance: DecryptableBalance =
        bytemuck::pod_read_unaligned(&new_decryptable_available_balance);

    let ix = inner_apply_pending_balance(
        &token_program_id,
        ctx.accounts.token_account.key,
        expected_pending_balance_credit_counter,
        &new_balance,
        ctx.accounts.owner.key,
        &[],
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.owner.to_account_info(),
        ],
    )?;

    Ok(())
}
