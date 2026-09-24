use anchor_lang::prelude::*;
use spl_token_2022_interface::extension::confidential_transfer::instruction::initialize_mint as initialize_confidential_transfer_mint;
use spl_token_2022_interface::extension::confidential_transfer_fee::instruction::initialize_confidential_transfer_fee_config;
use spl_token_2022_interface::extension::ExtensionType;
use spl_token_2022_interface::instruction::initialize_permanent_delegate;
use solana_zk_sdk_pod::encryption::elgamal::PodElGamalPubkey;

use super::create_mint::base_extensions;
use crate::mint_setup::create_mint_with_extensions;

// Task 5: re-issue the mint (confidential transfers can't be bolted on to an
// already-initialized one), carrying forward task 1's extension set and adding
// PermanentDelegate (seizure authority) + ConfidentialTransferMint (manual approve).
//
// The gap: token-2022 rejects InitializeMint2 if TransferFeeConfig and
// ConfidentialTransferMint are both present without a third extension,
// ConfidentialTransferFeeConfig -- an unencrypted fee would leak the hidden
// transfer amount, so the withheld fee has to be encrypted too, under its own
// ElGamal key (withdraw_withheld_authority_elgamal_pubkey below).
#[derive(Accounts)]
pub struct InitializeMintConfidential<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(mut)]
    pub mint: Signer<'info>,

    /// CHECK: address-constrained to the real Token-2022 program.
    #[account(address = spl_token_2022_interface::id())]
    pub token_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[allow(clippy::too_many_arguments)]
pub fn handle_initialize_mint_confidential(
    ctx: Context<InitializeMintConfidential>,
    decimals: u8,
    mint_authority: Pubkey,
    freeze_authority: Pubkey,
    transfer_fee_config_authority: Pubkey,
    withdraw_withheld_authority: Pubkey,
    transfer_fee_basis_points: u16,
    maximum_fee: u64,
    close_authority: Pubkey,
    permanent_delegate: Pubkey,
    confidential_transfer_authority: Pubkey,
    withdraw_withheld_authority_elgamal_pubkey: [u8; 32],
) -> Result<()> {
    let mint_key = ctx.accounts.mint.key();
    let token_program_id = spl_token_2022_interface::id();
    let withdraw_withheld_authority_elgamal_pubkey: PodElGamalPubkey =
        bytemuck::pod_read_unaligned(&withdraw_withheld_authority_elgamal_pubkey);

    let (base_types, base_ixs) = base_extensions(
        &token_program_id,
        &mint_key,
        mint_authority,
        transfer_fee_config_authority,
        withdraw_withheld_authority,
        transfer_fee_basis_points,
        maximum_fee,
        close_authority,
    )?;

    let extension_types: Vec<ExtensionType> = base_types
        .into_iter()
        .chain([
            ExtensionType::PermanentDelegate,
            ExtensionType::ConfidentialTransferMint,
            ExtensionType::ConfidentialTransferFeeConfig,
        ])
        .collect();

    let extension_init_ixs: Vec<_> = base_ixs
        .into_iter()
        .chain([
            initialize_permanent_delegate(&token_program_id, &mint_key, &permanent_delegate)?,
            initialize_confidential_transfer_mint(
                &token_program_id,
                &mint_key,
                Some(confidential_transfer_authority),
                false, // approve_policy = manual
                None,  // no third-party auditor key for this mint
            )?,
            initialize_confidential_transfer_fee_config(
                &token_program_id,
                &mint_key,
                Some(confidential_transfer_authority),
                &withdraw_withheld_authority_elgamal_pubkey,
            )?,
        ])
        .collect();

    create_mint_with_extensions(
        &ctx.accounts.payer.to_account_info(),
        &ctx.accounts.mint.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &token_program_id,
        &extension_types,
        &extension_init_ixs,
        &mint_authority,
        Some(&freeze_authority),
        decimals,
    )
}
