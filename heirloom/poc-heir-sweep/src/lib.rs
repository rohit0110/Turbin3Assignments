//! Minimal native Solana program used to test one specific, disputed claim:
//!
//! "A durable-nonce transaction can be signed by an owner *today*, calling a
//! custom instruction that takes no amount parameter and instead reads the
//! owner's live lamport balance at execution time, then submitted months
//! later (after the owner stops checking in) to sweep whatever balance
//! actually exists at that later point — not the balance that existed when
//! the signature was produced."
//!
//! This program deliberately does the minimum needed to exercise that claim:
//! `initialize` / `check_in` maintain a tiny liveness record, and
//! `heir_sweep` is the disputed instruction. See tests/integration.rs for
//! the actual proof (or disproof).

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint,
    entrypoint::ProgramResult,
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::Sysvar,
};

entrypoint!(process_instruction);

pub const VAULT_SEED: &[u8] = b"vault";

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct Vault {
    pub is_initialized: bool,
    pub owner: Pubkey,
    pub beneficiary: Pubkey,
    pub last_checkin: i64,
    pub timeout_secs: i64,
}

impl Vault {
    pub const LEN: usize = 1 + 32 + 32 + 8 + 8;
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum HeirInstruction {
    /// accounts: [owner (signer, writable), vault_pda (writable), system_program]
    Initialize { beneficiary: Pubkey, timeout_secs: i64 },
    /// accounts: [owner (signer), vault_pda (writable)]
    CheckIn,
    /// accounts: [owner (writable), beneficiary (writable), vault_pda (writable), system_program]
    ///
    /// No amount field. `owner` must be `is_signer == true` on the
    /// *transaction*, not on this instruction's own AccountMeta list — that
    /// signer-ness is what a durable-nonce presigned transaction is
    /// carrying. The amount swept is read live from `owner.lamports()`.
    HeirSweep,
}

#[derive(Debug)]
pub enum HeirError {
    NotInitialized,
    AlreadyInitialized,
    WrongOwner,
    OwnerNotSigner,
    TimeoutNotReached,
    InvalidVaultPda,
}

impl From<HeirError> for ProgramError {
    fn from(e: HeirError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

pub fn vault_pda(owner: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[VAULT_SEED, owner.as_ref()], program_id)
}

/// Fixed program id used only for the in-process `solana-program-test`
/// harness in tests/integration.rs -- this program is never deployed
/// on-chain, so no real base58 program id / keypair is needed.
pub fn id_for_tests() -> Pubkey {
    Pubkey::new_from_array([7u8; 32])
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let ix = HeirInstruction::try_from_slice(instruction_data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    match ix {
        HeirInstruction::Initialize {
            beneficiary,
            timeout_secs,
        } => initialize(program_id, accounts, beneficiary, timeout_secs),
        HeirInstruction::CheckIn => check_in(program_id, accounts),
        HeirInstruction::HeirSweep => heir_sweep(program_id, accounts),
    }
}

fn initialize(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    beneficiary: Pubkey,
    timeout_secs: i64,
) -> ProgramResult {
    let iter = &mut accounts.iter();
    let owner = next_account_info(iter)?;
    let vault_ai = next_account_info(iter)?;
    let system_program_ai = next_account_info(iter)?;

    if !owner.is_signer {
        return Err(HeirError::OwnerNotSigner.into());
    }

    let (expected_vault, bump) = vault_pda(owner.key, program_id);
    if expected_vault != *vault_ai.key {
        return Err(HeirError::InvalidVaultPda.into());
    }
    if vault_ai.data_len() > 0 {
        return Err(HeirError::AlreadyInitialized.into());
    }
    if !system_program::check_id(system_program_ai.key) {
        return Err(ProgramError::IncorrectProgramId);
    }

    let rent = Rent::get()?;
    let lamports = rent.minimum_balance(Vault::LEN);
    let seeds: &[&[u8]] = &[VAULT_SEED, owner.key.as_ref(), &[bump]];

    invoke_signed_create_account(
        owner,
        vault_ai,
        system_program_ai,
        program_id,
        lamports,
        Vault::LEN as u64,
        seeds,
    )?;

    let clock = Clock::get()?;
    let vault = Vault {
        is_initialized: true,
        owner: *owner.key,
        beneficiary,
        last_checkin: clock.unix_timestamp,
        timeout_secs,
    };
    vault.serialize(&mut &mut vault_ai.data.borrow_mut()[..])?;
    Ok(())
}

fn check_in(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let owner = next_account_info(iter)?;
    let vault_ai = next_account_info(iter)?;

    if !owner.is_signer {
        return Err(HeirError::OwnerNotSigner.into());
    }

    let (expected_vault, _bump) = vault_pda(owner.key, program_id);
    if expected_vault != *vault_ai.key {
        return Err(HeirError::InvalidVaultPda.into());
    }

    let mut vault = Vault::try_from_slice(&vault_ai.data.borrow())
        .map_err(|_| HeirError::NotInitialized)?;
    if !vault.is_initialized {
        return Err(HeirError::NotInitialized.into());
    }
    if vault.owner != *owner.key {
        return Err(HeirError::WrongOwner.into());
    }

    let clock = Clock::get()?;
    vault.last_checkin = clock.unix_timestamp;
    vault.serialize(&mut &mut vault_ai.data.borrow_mut()[..])?;
    Ok(())
}

/// The disputed instruction. No amount is ever encoded in the instruction
/// data or committed to by the signature — only "sweep whatever `owner` (a
/// plain System-owned wallet) holds, to `beneficiary`, if the vault's
/// liveness gate says the timeout has elapsed."
fn heir_sweep(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let owner = next_account_info(iter)?;
    let beneficiary = next_account_info(iter)?;
    let vault_ai = next_account_info(iter)?;
    let system_program_ai = next_account_info(iter)?;

    // This is the crux: `owner` was never given an AccountMeta by us marking
    // it a signer *for this instruction*. Its is_signer flag here is purely
    // a pass-through from the enclosing transaction's own signature check,
    // which the runtime performs once, before any instruction (or CPI)
    // runs. A durable-nonce transaction signed months ago carries that same
    // flag forward unchanged.
    if !owner.is_signer {
        return Err(HeirError::OwnerNotSigner.into());
    }

    let (expected_vault, _bump) = vault_pda(owner.key, program_id);
    if expected_vault != *vault_ai.key {
        return Err(HeirError::InvalidVaultPda.into());
    }

    let vault = Vault::try_from_slice(&vault_ai.data.borrow())
        .map_err(|_| HeirError::NotInitialized)?;
    if !vault.is_initialized {
        return Err(HeirError::NotInitialized.into());
    }
    if vault.owner != *owner.key {
        return Err(HeirError::WrongOwner.into());
    }
    if vault.beneficiary != *beneficiary.key {
        return Err(ProgramError::InvalidAccountData);
    }

    let clock = Clock::get()?;
    if clock.unix_timestamp < vault.last_checkin + vault.timeout_secs {
        return Err(HeirError::TimeoutNotReached.into());
    }

    // Read live, at execution time -- this is the whole point. Whatever the
    // balance was when the transaction was *signed* is irrelevant.
    let amount = owner.lamports();

    invoke(
        &system_instruction::transfer(owner.key, beneficiary.key, amount),
        &[owner.clone(), beneficiary.clone(), system_program_ai.clone()],
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn invoke_signed_create_account<'a>(
    payer: &AccountInfo<'a>,
    new_account: &AccountInfo<'a>,
    system_program_ai: &AccountInfo<'a>,
    owner_program: &Pubkey,
    lamports: u64,
    space: u64,
    signer_seeds: &[&[u8]],
) -> ProgramResult {
    solana_program::program::invoke_signed(
        &system_instruction::create_account(
            payer.key,
            new_account.key,
            lamports,
            space,
            owner_program,
        ),
        &[payer.clone(), new_account.clone(), system_program_ai.clone()],
        &[signer_seeds],
    )
}
