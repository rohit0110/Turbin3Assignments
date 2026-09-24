use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use solana_zk_sdk_pod::encryption::elgamal::PodElGamalCiphertext;
use spl_token_2022_interface::extension::confidential_transfer::instruction::inner_transfer_with_fee;
use spl_token_2022_interface::extension::confidential_transfer::DecryptableBalance;
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

// Task 6, step 4: confidential Transfer. Second gap: this mint carries
// TransferFeeConfig forward from task 1, and token-2022 rejects a plain
// Transfer on a fee-bearing mint (InvalidInstructionData) -- it has to be
// TransferWithFee, five proofs instead of three (adds fee-sigma and
// fee-ciphertext-validity, widens the range proof to batched-U256).
#[derive(Accounts)]
pub struct ConfidentialTransfer<'info> {
    pub authority: Signer<'info>,

    #[account(mut)]
    /// CHECK: ownership validated by the token-2022 CPI itself.
    pub source: UncheckedAccount<'info>,

    /// CHECK: only read for its pubkey.
    pub mint: UncheckedAccount<'info>,

    #[account(mut)]
    /// CHECK: ownership validated by the token-2022 CPI itself.
    pub destination: UncheckedAccount<'info>,

    /// CHECK: pre-verified proof context-state account.
    pub equality_proof_context_state: UncheckedAccount<'info>,

    /// CHECK: pre-verified proof context-state account.
    pub transfer_amount_validity_proof_context_state: UncheckedAccount<'info>,

    /// CHECK: pre-verified proof context-state account.
    pub fee_sigma_proof_context_state: UncheckedAccount<'info>,

    /// CHECK: pre-verified proof context-state account.
    pub fee_validity_proof_context_state: UncheckedAccount<'info>,

    /// CHECK: pre-verified proof context-state account.
    pub range_proof_context_state: UncheckedAccount<'info>,

    /// CHECK: address-constrained to the real Token-2022 program.
    #[account(address = spl_token_2022_interface::id())]
    pub token_program: UncheckedAccount<'info>,
}

pub fn handle_confidential_transfer(
    ctx: Context<ConfidentialTransfer>,
    new_source_decryptable_available_balance: [u8; 36],
    transfer_amount_auditor_ciphertext_lo: [u8; 64],
    transfer_amount_auditor_ciphertext_hi: [u8; 64],
) -> Result<()> {
    let token_program_id = spl_token_2022_interface::id();
    let new_balance: DecryptableBalance =
        bytemuck::pod_read_unaligned(&new_source_decryptable_available_balance);
    let ct_lo: PodElGamalCiphertext =
        bytemuck::pod_read_unaligned(&transfer_amount_auditor_ciphertext_lo);
    let ct_hi: PodElGamalCiphertext =
        bytemuck::pod_read_unaligned(&transfer_amount_auditor_ciphertext_hi);

    let equality_key = ctx.accounts.equality_proof_context_state.key();
    let transfer_validity_key = ctx
        .accounts
        .transfer_amount_validity_proof_context_state
        .key();
    let fee_sigma_key = ctx.accounts.fee_sigma_proof_context_state.key();
    let fee_validity_key = ctx.accounts.fee_validity_proof_context_state.key();
    let range_key = ctx.accounts.range_proof_context_state.key();

    let ix = inner_transfer_with_fee(
        &token_program_id,
        ctx.accounts.source.key,
        ctx.accounts.mint.key,
        ctx.accounts.destination.key,
        &new_balance,
        &ct_lo,
        &ct_hi,
        ctx.accounts.authority.key,
        &[],
        ProofLocation::ContextStateAccount(&equality_key),
        ProofLocation::ContextStateAccount(&transfer_validity_key),
        ProofLocation::ContextStateAccount(&fee_sigma_key),
        ProofLocation::ContextStateAccount(&fee_validity_key),
        ProofLocation::ContextStateAccount(&range_key),
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.source.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.destination.to_account_info(),
            ctx.accounts.equality_proof_context_state.to_account_info(),
            ctx.accounts
                .transfer_amount_validity_proof_context_state
                .to_account_info(),
            ctx.accounts.fee_sigma_proof_context_state.to_account_info(),
            ctx.accounts
                .fee_validity_proof_context_state
                .to_account_info(),
            ctx.accounts.range_proof_context_state.to_account_info(),
            ctx.accounts.authority.to_account_info(),
        ],
    )?;

    Ok(())
}
