//! Task 5 + 6: re-issue the mint with a seizure authority and confidential
//! transfers, then drive the full confidential lifecycle with real ZK
//! proofs verified on-chain by devnet's ZK ElGamal proof program.

use std::mem::size_of;

use anchor_lang::{
    solana_program::{instruction::Instruction, system_instruction, system_program},
    InstructionData, ToAccountMetas,
};
use anyhow::Result;
use bytemuck::Pod;
use solana_client::rpc_client::RpcClient;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_zk_elgamal_proof_interface::{
    self as zk_elgamal_proof_program,
    instruction::{close_context_state, ContextStateInfo, ProofInstruction},
    proof_data::{
        BatchedGroupedCiphertext2HandlesValidityProofContext,
        BatchedGroupedCiphertext3HandlesValidityProofContext, BatchedRangeProofContext,
        CiphertextCommitmentEqualityProofContext, PercentageWithCapProofContext,
        PubkeyValidityProofContext, ZkProofData,
    },
    state::ProofContextState,
};
use solana_zk_sdk::{
    encryption::{
        auth_encryption::{AeCiphertext, AeKey},
        elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
    },
    encryption::derivation::derive_confidential_keys,
    zk_elgamal_proof_program::build_pubkey_validity_proof_data,
};
use solana_zk_sdk_pod::encryption::{auth_encryption::PodAeCiphertext, elgamal::PodElGamalPubkey};
use spl_token_2022_interface::extension::{
    confidential_transfer::{instruction as ct_ix, ConfidentialTransferAccount},
    BaseStateWithExtensions, StateWithExtensions,
};
use spl_token_2022_interface::state::Account as TokenAccount;

use crate::{create_ata, mint_to, send, thaw, token_program_id};

const DEFAULT_MAX_PENDING_BALANCE_CREDIT_COUNTER: u64 = 65_536;

// same derivation spl-token-cli uses: deterministic, from a signature over the token account's address
fn derive_keys(owner: &Keypair, token_account: &Pubkey) -> Result<(ElGamalKeypair, AeKey)> {
    derive_confidential_keys(owner, token_account.as_ref()).map_err(|e| anyhow::anyhow!("{e}"))
}

fn pod_bytes<T: Pod, const N: usize>(value: &T) -> [u8; N] {
    bytemuck::bytes_of(value).try_into().expect("size mismatch")
}

fn read_confidential_account(rpc: &RpcClient, ata: &Pubkey) -> Result<ConfidentialTransferAccount> {
    let data = rpc.get_account_data(ata)?;
    let state = StateWithExtensions::<TokenAccount>::unpack(&data)?;
    Ok(*state.get_extension::<ConfidentialTransferAccount>()?)
}

fn verify_proof_into_context<T, U>(
    rpc: &RpcClient,
    payer: &Keypair,
    proof_data: &T,
    verify_instruction: ProofInstruction,
) -> Result<Keypair>
where
    T: Pod + ZkProofData<U>,
    U: Pod,
{
    let ctx_kp = Keypair::new();
    let space = size_of::<ProofContextState<U>>();
    let rent = rpc.get_minimum_balance_for_rent_exemption(space)?;

    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &ctx_kp.pubkey(),
        rent,
        space as u64,
        &zk_elgamal_proof_program::id(),
    );
    let verify_ix = verify_instruction.encode_verify_proof(
        Some(ContextStateInfo {
            context_state_account: &ctx_kp.pubkey(),
            context_state_authority: &payer.pubkey(),
        }),
        proof_data,
    );
    // split across two txs -- create + verify together can exceed the 1232-byte limit
    send(rpc, &[create_ix], &payer.pubkey(), &[payer, &ctx_kp])?;
    send(rpc, &[verify_ix], &payer.pubkey(), &[payer])?;
    Ok(ctx_kp)
}

fn close_context(rpc: &RpcClient, payer: &Keypair, context_account: &Pubkey) -> Result<()> {
    let ix = close_context_state(
        ContextStateInfo {
            context_state_account: context_account,
            context_state_authority: &payer.pubkey(),
        },
        &payer.pubkey(),
    );
    send(rpc, &[ix], &payer.pubkey(), &[payer])
}

// the U256 range proof alone is too big to embed inline; stage it in an
// spl-record account first (same fix spl-token-cli uses), then verify from that account
fn upload_proof_record<T: Pod>(rpc: &RpcClient, payer: &Keypair, proof_data: &T) -> Result<Keypair> {
    let record_kp = Keypair::new();
    let bytes = bytemuck::bytes_of(proof_data);
    let space = spl_record::state::RecordData::WRITABLE_START_INDEX + bytes.len();
    let rent = rpc.get_minimum_balance_for_rent_exemption(space)?;

    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &record_kp.pubkey(),
        rent,
        space as u64,
        &spl_record::id(),
    );
    let init_ix = spl_record::instruction::initialize(&record_kp.pubkey(), &payer.pubkey());
    send(rpc, &[create_ix, init_ix], &payer.pubkey(), &[payer, &record_kp])?;

    const CHUNK: usize = 900;
    for (i, chunk) in bytes.chunks(CHUNK).enumerate() {
        let offset = (i * CHUNK) as u64;
        let write_ix =
            spl_record::instruction::write(&record_kp.pubkey(), &payer.pubkey(), offset, chunk);
        send(rpc, &[write_ix], &payer.pubkey(), &[payer])?;
    }
    Ok(record_kp)
}

fn close_proof_record(rpc: &RpcClient, payer: &Keypair, record_account: &Pubkey) -> Result<()> {
    let ix = spl_record::instruction::close_account(record_account, &payer.pubkey(), &payer.pubkey());
    send(rpc, &[ix], &payer.pubkey(), &[payer])
}

fn verify_large_proof_into_context<T, U>(
    rpc: &RpcClient,
    payer: &Keypair,
    proof_data: &T,
    verify_instruction: ProofInstruction,
) -> Result<Keypair>
where
    T: Pod,
    U: Pod,
{
    let record_kp = upload_proof_record(rpc, payer, proof_data)?;

    let ctx_kp = Keypair::new();
    let space = size_of::<ProofContextState<U>>();
    let rent = rpc.get_minimum_balance_for_rent_exemption(space)?;
    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &ctx_kp.pubkey(),
        rent,
        space as u64,
        &zk_elgamal_proof_program::id(),
    );
    send(rpc, &[create_ix], &payer.pubkey(), &[payer, &ctx_kp])?;

    let verify_ix = verify_instruction.encode_verify_proof_from_account(
        Some(ContextStateInfo {
            context_state_account: &ctx_kp.pubkey(),
            context_state_authority: &payer.pubkey(),
        }),
        &record_kp.pubkey(),
        spl_record::state::RecordData::WRITABLE_START_INDEX as u32,
    );
    send(rpc, &[verify_ix], &payer.pubkey(), &[payer])?;

    close_proof_record(rpc, payer, &record_kp.pubkey())?;
    Ok(ctx_kp)
}

// task 5: re-issue the mint

#[allow(clippy::too_many_arguments)]
fn initialize_mint_confidential(
    rpc: &RpcClient,
    payer: &Keypair,
    mint: &Keypair,
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
    let ix = Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::InitializeMintConfidential {
            decimals,
            mint_authority,
            freeze_authority,
            transfer_fee_config_authority,
            withdraw_withheld_authority,
            transfer_fee_basis_points,
            maximum_fee,
            close_authority,
            permanent_delegate,
            confidential_transfer_authority,
            withdraw_withheld_authority_elgamal_pubkey,
        }
        .data(),
        stablecoin::accounts::InitializeMintConfidential {
            payer: payer.pubkey(),
            mint: mint.pubkey(),
            token_program: token_program_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    send(rpc, &[ix], &payer.pubkey(), &[payer, mint])
}

// task 6: the confidential lifecycle

fn configure_confidential_account(
    rpc: &RpcClient,
    payer: &Keypair,
    owner: &Keypair,
    ata: &Pubkey,
    mint: &Pubkey,
    elgamal_keypair: &ElGamalKeypair,
    aes_key: &AeKey,
) -> Result<()> {
    let proof_data =
        build_pubkey_validity_proof_data(elgamal_keypair).map_err(|e| anyhow::anyhow!("{e}"))?;
    let ctx_kp = verify_proof_into_context::<_, PubkeyValidityProofContext>(
        rpc,
        payer,
        &proof_data,
        ProofInstruction::VerifyPubkeyValidity,
    )?;

    // ConfigureAccount doesn't grow the account itself -- needs a prior Reallocate
    let realloc_ix = spl_token_2022_interface::instruction::reallocate(
        &token_program_id(),
        ata,
        &owner.pubkey(),
        &owner.pubkey(),
        &[],
        &[
            spl_token_2022_interface::extension::ExtensionType::ConfidentialTransferAccount,
            spl_token_2022_interface::extension::ExtensionType::ConfidentialTransferFeeAmount,
        ],
    )?;

    let zero_balance: PodAeCiphertext = aes_key.encrypt(0).into();
    let configure_ix = Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::ConfigureConfidentialAccount {
            decryptable_zero_balance: pod_bytes(&zero_balance),
            maximum_pending_balance_credit_counter: DEFAULT_MAX_PENDING_BALANCE_CREDIT_COUNTER,
        }
        .data(),
        stablecoin::accounts::ConfigureConfidentialAccount {
            owner: owner.pubkey(),
            token_account: *ata,
            mint: *mint,
            proof_context_state: ctx_kp.pubkey(),
            token_program: token_program_id(),
        }
        .to_account_metas(None),
    );
    send(rpc, &[realloc_ix, configure_ix], &owner.pubkey(), &[owner])?;

    close_context(rpc, payer, &ctx_kp.pubkey())
}

// manual approve_policy means a freshly configured account starts unapproved
fn approve_confidential_account(
    rpc: &RpcClient,
    confidential_transfer_authority: &Keypair,
    ata: &Pubkey,
    mint: &Pubkey,
) -> Result<()> {
    let ix = ct_ix::approve_account(
        &token_program_id(),
        ata,
        mint,
        &confidential_transfer_authority.pubkey(),
        &[],
    )?;
    send(rpc, &[ix], &confidential_transfer_authority.pubkey(), &[confidential_transfer_authority])
}

fn deposit_confidential(
    rpc: &RpcClient,
    authority: &Keypair,
    ata: &Pubkey,
    mint: &Pubkey,
    amount: u64,
    decimals: u8,
) -> Result<()> {
    let ix = Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::DepositConfidential { amount, decimals }.data(),
        stablecoin::accounts::DepositConfidential {
            authority: authority.pubkey(),
            token_account: *ata,
            mint: *mint,
            token_program: token_program_id(),
        }
        .to_account_metas(None),
    );
    send(rpc, &[ix], &authority.pubkey(), &[authority])
}

fn apply_pending_balance(
    rpc: &RpcClient,
    owner: &Keypair,
    ata: &Pubkey,
    aes_key: &AeKey,
    new_available_balance: u64,
) -> Result<()> {
    let account = read_confidential_account(rpc, ata)?;
    let new_balance: PodAeCiphertext = aes_key.encrypt(new_available_balance).into();

    let ix = Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::ApplyPendingBalance {
            expected_pending_balance_credit_counter: u64::from(
                account.pending_balance_credit_counter,
            ),
            new_decryptable_available_balance: pod_bytes(&new_balance),
        }
        .data(),
        stablecoin::accounts::ApplyPendingBalance {
            owner: owner.pubkey(),
            token_account: *ata,
            token_program: token_program_id(),
        }
        .to_account_metas(None),
    );
    send(rpc, &[ix], &owner.pubkey(), &[owner])
}

// this mint carries TransferFeeConfig forward, so it has to be TransferWithFee (5 proofs, not 3)
#[allow(clippy::too_many_arguments)]
fn confidential_transfer(
    rpc: &RpcClient,
    payer: &Keypair,
    authority: &Keypair,
    source: &Pubkey,
    destination: &Pubkey,
    mint: &Pubkey,
    source_elgamal_keypair: &ElGamalKeypair,
    source_aes_key: &AeKey,
    destination_elgamal_pubkey: &ElGamalPubkey,
    withdraw_withheld_authority_elgamal_pubkey: &ElGamalPubkey,
    fee_rate_basis_points: u16,
    maximum_fee: u64,
    transfer_amount: u64,
    source_balance_after: u64,
) -> Result<()> {
    let source_account = read_confidential_account(rpc, source)?;
    let current_available_balance: ElGamalCiphertext = source_account
        .available_balance
        .try_into()
        .map_err(|e: solana_zk_sdk::errors::ElGamalError| anyhow::anyhow!("{e}"))?;
    let current_decryptable_available_balance: AeCiphertext = source_account
        .decryptable_available_balance
        .try_into()
        .map_err(|e: solana_zk_sdk::errors::AuthenticatedEncryptionError| {
            anyhow::anyhow!("{e}")
        })?;

    let proof = spl_token_confidential_transfer_proof_generation::transfer_with_fee::transfer_with_fee_split_proof_data(
        &current_available_balance,
        &current_decryptable_available_balance,
        transfer_amount,
        source_elgamal_keypair,
        source_aes_key,
        destination_elgamal_pubkey,
        None,
        withdraw_withheld_authority_elgamal_pubkey,
        fee_rate_basis_points,
        maximum_fee,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let equality_ctx = verify_proof_into_context::<_, CiphertextCommitmentEqualityProofContext>(
        rpc,
        payer,
        &proof.equality_proof_data,
        ProofInstruction::VerifyCiphertextCommitmentEquality,
    )?;
    let transfer_validity_ctx =
        verify_proof_into_context::<_, BatchedGroupedCiphertext3HandlesValidityProofContext>(
            rpc,
            payer,
            &proof
                .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
                .proof_data,
            ProofInstruction::VerifyBatchedGroupedCiphertext3HandlesValidity,
        )?;
    let fee_sigma_ctx = verify_proof_into_context::<_, PercentageWithCapProofContext>(
        rpc,
        payer,
        &proof.percentage_with_cap_proof_data,
        ProofInstruction::VerifyPercentageWithCap,
    )?;
    let fee_validity_ctx =
        verify_proof_into_context::<_, BatchedGroupedCiphertext2HandlesValidityProofContext>(
            rpc,
            payer,
            &proof.fee_ciphertext_validity_proof_data,
            ProofInstruction::VerifyBatchedGroupedCiphertext2HandlesValidity,
        )?;
    let range_ctx = verify_large_proof_into_context::<_, BatchedRangeProofContext>(
        rpc,
        payer,
        &proof.range_proof_data,
        ProofInstruction::VerifyBatchedRangeProofU256,
    )?;

    let new_source_balance: PodAeCiphertext = source_aes_key.encrypt(source_balance_after).into();
    let ix = Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::ConfidentialTransfer {
            new_source_decryptable_available_balance: pod_bytes(&new_source_balance),
            transfer_amount_auditor_ciphertext_lo: pod_bytes(
                &proof
                    .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
                    .ciphertext_lo,
            ),
            transfer_amount_auditor_ciphertext_hi: pod_bytes(
                &proof
                    .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
                    .ciphertext_hi,
            ),
        }
        .data(),
        stablecoin::accounts::ConfidentialTransfer {
            authority: authority.pubkey(),
            source: *source,
            mint: *mint,
            destination: *destination,
            equality_proof_context_state: equality_ctx.pubkey(),
            transfer_amount_validity_proof_context_state: transfer_validity_ctx.pubkey(),
            fee_sigma_proof_context_state: fee_sigma_ctx.pubkey(),
            fee_validity_proof_context_state: fee_validity_ctx.pubkey(),
            range_proof_context_state: range_ctx.pubkey(),
            token_program: token_program_id(),
        }
        .to_account_metas(None),
    );
    send(rpc, &[ix], &authority.pubkey(), &[authority])?;

    close_context(rpc, payer, &equality_ctx.pubkey())?;
    close_context(rpc, payer, &transfer_validity_ctx.pubkey())?;
    close_context(rpc, payer, &fee_sigma_ctx.pubkey())?;
    close_context(rpc, payer, &fee_validity_ctx.pubkey())?;
    close_context(rpc, payer, &range_ctx.pubkey())
}

#[allow(clippy::too_many_arguments)]
fn withdraw_confidential(
    rpc: &RpcClient,
    payer: &Keypair,
    authority: &Keypair,
    ata: &Pubkey,
    mint: &Pubkey,
    elgamal_keypair: &ElGamalKeypair,
    aes_key: &AeKey,
    current_balance: u64,
    withdraw_amount: u64,
    decimals: u8,
) -> Result<()> {
    let account = read_confidential_account(rpc, ata)?;
    let current_available_balance: ElGamalCiphertext = account
        .available_balance
        .try_into()
        .map_err(|e: solana_zk_sdk::errors::ElGamalError| anyhow::anyhow!("{e}"))?;

    let proof = spl_token_confidential_transfer_proof_generation::withdraw::withdraw_proof_data(
        &current_available_balance,
        current_balance,
        withdraw_amount,
        elgamal_keypair,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let equality_ctx = verify_proof_into_context::<_, CiphertextCommitmentEqualityProofContext>(
        rpc,
        payer,
        &proof.equality_proof_data,
        ProofInstruction::VerifyCiphertextCommitmentEquality,
    )?;
    let range_ctx = verify_proof_into_context::<_, BatchedRangeProofContext>(
        rpc,
        payer,
        &proof.range_proof_data,
        ProofInstruction::VerifyBatchedRangeProofU64,
    )?;

    let remaining = current_balance - withdraw_amount;
    let new_balance: PodAeCiphertext = aes_key.encrypt(remaining).into();

    let ix = Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::WithdrawConfidential {
            amount: withdraw_amount,
            decimals,
            new_decryptable_available_balance: pod_bytes(&new_balance),
        }
        .data(),
        stablecoin::accounts::WithdrawConfidential {
            authority: authority.pubkey(),
            token_account: *ata,
            mint: *mint,
            equality_proof_context_state: equality_ctx.pubkey(),
            range_proof_context_state: range_ctx.pubkey(),
            token_program: token_program_id(),
        }
        .to_account_metas(None),
    );
    send(rpc, &[ix], &authority.pubkey(), &[authority])?;

    close_context(rpc, payer, &equality_ctx.pubkey())?;
    close_context(rpc, payer, &range_ctx.pubkey())
}

pub fn run(rpc: &RpcClient, payer: &Keypair, alice: &Keypair, bob: &Keypair) -> Result<()> {
    println!("\n=== Task 5: initialize_mint_confidential (seizure + confidential transfers) ===");
    let mint = Keypair::new();
    let (withdraw_withheld_elgamal, _withdraw_withheld_aes) =
        derive_confidential_keys(payer, mint.pubkey().as_ref()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let withdraw_withheld_pubkey_bytes: [u8; 32] =
        pod_bytes(&PodElGamalPubkey::from(*withdraw_withheld_elgamal.pubkey()));

    initialize_mint_confidential(
        rpc,
        payer,
        &mint,
        6,
        payer.pubkey(),
        payer.pubkey(),
        payer.pubkey(),
        payer.pubkey(),
        100,
        1_000,
        payer.pubkey(),
        payer.pubkey(), // permanent delegate = payer, for this demo
        payer.pubkey(), // confidential-transfer authority
        withdraw_withheld_pubkey_bytes,
    )?;
    println!("confidential mint: {}", mint.pubkey());

    let alice_ata = create_ata(rpc, payer, &alice.pubkey(), &mint.pubkey())?;
    let bob_ata = create_ata(rpc, payer, &bob.pubkey(), &mint.pubkey())?;
    thaw(rpc, payer, &alice_ata, &mint.pubkey())?;
    thaw(rpc, payer, &bob_ata, &mint.pubkey())?;
    mint_to(rpc, payer, &mint.pubkey(), &alice_ata, 1_000_000)?;
    println!("alice_ata: {alice_ata}\nbob_ata:   {bob_ata}");

    println!("\n=== Task 6, step 1: ConfigureAccount (owner-only, real PubkeyValidityProof) ===");
    let (alice_elgamal, alice_aes) = derive_keys(alice, &alice_ata)?;
    let (bob_elgamal, bob_aes) = derive_keys(bob, &bob_ata)?;
    configure_confidential_account(rpc, payer, alice, &alice_ata, &mint.pubkey(), &alice_elgamal, &alice_aes)?;
    configure_confidential_account(rpc, payer, bob, &bob_ata, &mint.pubkey(), &bob_elgamal, &bob_aes)?;

    approve_confidential_account(rpc, payer, &alice_ata, &mint.pubkey())?;
    approve_confidential_account(rpc, payer, &bob_ata, &mint.pubkey())?;

    println!("\n=== Task 6, step 2: DepositConfidentialTokens (no proof needed) ===");
    let deposit_amount = 400_000u64;
    deposit_confidential(rpc, alice, &alice_ata, &mint.pubkey(), deposit_amount, 6)?;

    println!("\n=== Task 6, step 3: ApplyPendingBalance ===");
    apply_pending_balance(rpc, alice, &alice_ata, &alice_aes, deposit_amount)?;

    println!("\n=== Task 6, step 4: confidential TransferWithFee (equality + transfer-validity + fee-sigma + fee-validity + range proofs) ===");
    let transfer_amount = 150_000u64;
    let fee_rate_basis_points: u16 = 100;
    let max_fee: u64 = 1_000;
    let confidential_fee = max_fee.min(transfer_amount * fee_rate_basis_points as u64 / 10_000);
    let alice_balance_after_transfer = deposit_amount - transfer_amount;
    let bob_pending_after_transfer = transfer_amount - confidential_fee;
    confidential_transfer(
        rpc,
        payer,
        alice,
        &alice_ata,
        &bob_ata,
        &mint.pubkey(),
        &alice_elgamal,
        &alice_aes,
        bob_elgamal.pubkey(),
        withdraw_withheld_elgamal.pubkey(),
        fee_rate_basis_points,
        max_fee,
        transfer_amount,
        alice_balance_after_transfer,
    )?;

    println!("\n=== Task 6, step 5: apply pending, then WithdrawConfidentialTokens ===");
    apply_pending_balance(rpc, bob, &bob_ata, &bob_aes, bob_pending_after_transfer)?;
    let withdraw_amount = 50_000u64;
    withdraw_confidential(
        rpc,
        payer,
        bob,
        &bob_ata,
        &mint.pubkey(),
        &bob_elgamal,
        &bob_aes,
        bob_pending_after_transfer,
        withdraw_amount,
        6,
    )?;

    println!(
        "\nbob's public balance after withdrawing {withdraw_amount} confidential units: {}",
        crate::token_balance(rpc, &bob_ata)?
    );
    println!(
        "(the remaining {} stay confidential, visible only to bob's own keys)",
        bob_pending_after_transfer - withdraw_amount
    );

    Ok(())
}
