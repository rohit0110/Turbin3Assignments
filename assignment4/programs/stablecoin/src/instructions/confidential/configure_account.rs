use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use spl_token_2022_interface::extension::confidential_transfer::instruction::inner_configure_account;
use spl_token_2022_interface::extension::confidential_transfer::DecryptableBalance;
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

// Task 6, step 1: ConfigureAccount. Owner-only, unlike ATA creation which
// anyone can pay for. The PubkeyValidityProof is verified ahead of time into
// proof_context_state by the client (ContextStateAccount pattern), since
// we're reached via CPI here.
#[derive(Accounts)]
pub struct ConfigureConfidentialAccount<'info> {
    pub owner: Signer<'info>,

    #[account(mut)]
    /// CHECK: ownership validated by the token-2022 CPI itself.
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: only read for its pubkey.
    pub mint: UncheckedAccount<'info>,

    /// CHECK: a `PubkeyValidityProofData` context-state account, already
    /// verified by the ZK ElGamal proof program in a prior transaction.
    pub proof_context_state: UncheckedAccount<'info>,

    /// CHECK: address-constrained to the real Token-2022 program.
    #[account(address = spl_token_2022_interface::id())]
    pub token_program: UncheckedAccount<'info>,
}

pub fn handle_configure_confidential_account(
    ctx: Context<ConfigureConfidentialAccount>,
    decryptable_zero_balance: [u8; 36],
    maximum_pending_balance_credit_counter: u64,
) -> Result<()> {
    let token_program_id = spl_token_2022_interface::id();
    let decryptable_zero_balance: DecryptableBalance =
        bytemuck::pod_read_unaligned(&decryptable_zero_balance);
    let proof_context_key = ctx.accounts.proof_context_state.key();

    let ix = inner_configure_account(
        &token_program_id,
        ctx.accounts.token_account.key,
        ctx.accounts.mint.key,
        &decryptable_zero_balance,
        maximum_pending_balance_credit_counter,
        ctx.accounts.owner.key,
        &[],
        ProofLocation::ContextStateAccount(&proof_context_key),
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.proof_context_state.to_account_info(),
            ctx.accounts.owner.to_account_info(),
        ],
    )?;

    Ok(())
}
