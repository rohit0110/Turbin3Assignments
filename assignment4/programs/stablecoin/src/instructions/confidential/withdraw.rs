use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use spl_token_2022_interface::extension::confidential_transfer::instruction::inner_withdraw;
use spl_token_2022_interface::extension::confidential_transfer::DecryptableBalance;
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

// Task 6, step 5: WithdrawConfidentialTokens. Spends only from the available
// balance, never pending -- apply_pending_balance must run first if the
// withdrawal needs to draw on a recent deposit or transfer.
#[derive(Accounts)]
pub struct WithdrawConfidential<'info> {
    pub authority: Signer<'info>,

    #[account(mut)]
    /// CHECK: ownership validated by the token-2022 CPI itself.
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: only read for its pubkey.
    pub mint: UncheckedAccount<'info>,

    /// CHECK: pre-verified proof context-state account.
    pub equality_proof_context_state: UncheckedAccount<'info>,

    /// CHECK: pre-verified proof context-state account.
    pub range_proof_context_state: UncheckedAccount<'info>,

    /// CHECK: address-constrained to the real Token-2022 program.
    #[account(address = spl_token_2022_interface::id())]
    pub token_program: UncheckedAccount<'info>,
}

pub fn handle_withdraw_confidential(
    ctx: Context<WithdrawConfidential>,
    amount: u64,
    decimals: u8,
    new_decryptable_available_balance: [u8; 36],
) -> Result<()> {
    let token_program_id = spl_token_2022_interface::id();
    let new_balance: DecryptableBalance =
        bytemuck::pod_read_unaligned(&new_decryptable_available_balance);

    let equality_key = ctx.accounts.equality_proof_context_state.key();
    let range_key = ctx.accounts.range_proof_context_state.key();

    let ix = inner_withdraw(
        &token_program_id,
        ctx.accounts.token_account.key,
        ctx.accounts.mint.key,
        amount,
        decimals,
        &new_balance,
        ctx.accounts.authority.key,
        &[],
        ProofLocation::ContextStateAccount(&equality_key),
        ProofLocation::ContextStateAccount(&range_key),
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.equality_proof_context_state.to_account_info(),
            ctx.accounts.range_proof_context_state.to_account_info(),
            ctx.accounts.authority.to_account_info(),
        ],
    )?;

    Ok(())
}
