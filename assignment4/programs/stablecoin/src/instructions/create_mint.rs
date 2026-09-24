use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::Instruction;
use spl_token_2022_interface::extension::default_account_state::instruction::initialize_default_account_state;
use spl_token_2022_interface::extension::metadata_pointer::instruction::initialize as initialize_metadata_pointer;
use spl_token_2022_interface::extension::transfer_fee::instruction::initialize_transfer_fee_config;
use spl_token_2022_interface::extension::ExtensionType;
use spl_token_2022_interface::instruction::initialize_mint_close_authority;
use spl_token_2022_interface::state::AccountState;

use crate::mint_setup::create_mint_with_extensions;

// Shared by task 1's InitializeMint and task 5's InitializeMintConfidential.
#[allow(clippy::too_many_arguments)]
pub fn base_extensions(
    token_program_id: &Pubkey,
    mint_key: &Pubkey,
    mint_authority: Pubkey,
    transfer_fee_config_authority: Pubkey,
    withdraw_withheld_authority: Pubkey,
    transfer_fee_basis_points: u16,
    maximum_fee: u64,
    close_authority: Pubkey,
) -> Result<([ExtensionType; 4], [Instruction; 4])> {
    let extension_types = [
        ExtensionType::TransferFeeConfig,
        ExtensionType::MetadataPointer,
        ExtensionType::DefaultAccountState,
        ExtensionType::MintCloseAuthority,
    ];

    // must all land before InitializeMint2
    let extension_init_ixs = [
        initialize_transfer_fee_config(
            token_program_id,
            mint_key,
            Some(&transfer_fee_config_authority),
            Some(&withdraw_withheld_authority),
            transfer_fee_basis_points,
            maximum_fee,
        )?,
        initialize_metadata_pointer(
            token_program_id,
            mint_key,
            Some(mint_authority),
            Some(*mint_key),
        )?,
        initialize_default_account_state(token_program_id, mint_key, &AccountState::Frozen)?,
        initialize_mint_close_authority(token_program_id, mint_key, Some(&close_authority))?,
    ];

    Ok((extension_types, extension_init_ixs))
}

// Task 1: mint stacking TransferFeeConfig, MetadataPointer, DefaultAccountState::Frozen,
// and MintCloseAuthority. freeze_authority is the KYC authority that thaws accounts later.
#[derive(Accounts)]
pub struct InitializeMint<'info> {
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
pub fn handle_initialize_mint(
    ctx: Context<InitializeMint>,
    decimals: u8,
    mint_authority: Pubkey,
    freeze_authority: Pubkey,
    transfer_fee_config_authority: Pubkey,
    withdraw_withheld_authority: Pubkey,
    transfer_fee_basis_points: u16,
    maximum_fee: u64,
    close_authority: Pubkey,
) -> Result<()> {
    let mint_key = ctx.accounts.mint.key();
    let token_program_id = spl_token_2022_interface::id();

    let (extension_types, extension_init_ixs) = base_extensions(
        &token_program_id,
        &mint_key,
        mint_authority,
        transfer_fee_config_authority,
        withdraw_withheld_authority,
        transfer_fee_basis_points,
        maximum_fee,
        close_authority,
    )?;

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
