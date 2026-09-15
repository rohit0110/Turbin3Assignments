//! Real-validator proof for the disputed claim in
//! capstone/deliverable-1-loi.md: "sign a durable-nonce transaction today,
//! calling a no-amount `heir_sweep` instruction that reads the owner's live
//! balance at execution time, hold the bytes, submit months later."
//!
//! This has to run against an actual `solana-test-validator`, not
//! `solana-program-test`'s in-process bank -- that in-process harness
//! literally cannot submit a durable-nonce transaction; its banks-server
//! does an unconditional `get_blockhash_last_valid_block_height(recent_blockhash).unwrap()`
//! that assumes `recent_blockhash` is always a real, currently-queued
//! blockhash, which panics for a nonce-derived hash (see README.md).
//!
//! Setup (see README.md for the one-time build/deploy steps):
//!   solana-test-validator --reset --quiet &
//!   solana program deploy target/deploy/heir_sweep_poc.so \
//!       --program-id target/deploy/heir_sweep_poc-keypair.json
//!   cargo run --example durable_nonce_demo
//!
//! Uses an 8-second vault timeout and real wall-clock sleeps -- short enough
//! to run in seconds, long enough to be an honest stand-in for "months of
//! silence" as far as the mechanism is concerned: nothing here depends on
//! the actual duration, only on Clock and the nonce account's state.

use heir_sweep_poc::{vault_pda, HeirInstruction};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_program::system_program;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::Message,
    nonce,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    system_instruction,
    transaction::Transaction,
};
use std::{thread::sleep, time::Duration};

/// Sends WITHOUT client-side preflight simulation -- exactly what a naive
/// keeper script, a relayer racing to be first, or an adversarial holder of
/// the bytes would do, and unlike `send_and_confirm_transaction`, this really
/// puts the transaction in front of the leader even when it's known to fail.
/// Returns the on-chain execution result once the tx actually lands.
fn send_skip_preflight_and_wait(rpc: &RpcClient, tx: &Transaction) -> (Signature, Option<solana_sdk::transaction::Result<()>>) {
    let config = RpcSendTransactionConfig {
        skip_preflight: true,
        ..Default::default()
    };
    let sig = rpc.send_transaction_with_config(tx, config).unwrap();
    for _ in 0..100 {
        if let Ok(statuses) = rpc.get_signature_statuses(&[sig]) {
            if let Some(Some(status)) = statuses.value.into_iter().next() {
                return (sig, Some(status.status));
            }
        }
        sleep(Duration::from_millis(200));
    }
    (sig, None)
}

const RPC_URL: &str = "http://127.0.0.1:8899";
const TIMEOUT_SECS: i64 = 8;

fn heir_ix(
    program_id: &Pubkey,
    owner: &Pubkey,
    beneficiary: &Pubkey,
    vault: &Pubkey,
    data: HeirInstruction,
) -> Instruction {
    let accounts = match data {
        HeirInstruction::Initialize { .. } => vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new(*vault, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        HeirInstruction::CheckIn => vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new(*vault, false),
        ],
        HeirInstruction::HeirSweep => vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new(*beneficiary, false),
            AccountMeta::new(*vault, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    };
    Instruction {
        program_id: *program_id,
        accounts,
        data: borsh::to_vec(&data).unwrap(),
    }
}

fn decode_durable_nonce(data: &[u8]) -> Hash {
    let versions: nonce::state::Versions = bincode::deserialize(data).unwrap();
    match versions.state() {
        nonce::state::State::Initialized(d) => Hash::from(*d.durable_nonce.as_hash()),
        nonce::state::State::Uninitialized => panic!("nonce account not initialized"),
    }
}

fn airdrop(rpc: &RpcClient, to: &Pubkey, lamports: u64) {
    let sig = rpc.request_airdrop(to, lamports).unwrap();
    loop {
        if rpc.confirm_transaction(&sig).unwrap() {
            break;
        }
        sleep(Duration::from_millis(200));
    }
}

fn create_nonce_account(rpc: &RpcClient, owner: &Keypair, nonce_account: &Keypair) {
    let rent = rpc
        .get_minimum_balance_for_rent_exemption(nonce::State::size())
        .unwrap();
    let ixs = system_instruction::create_nonce_account(
        &owner.pubkey(),
        &nonce_account.pubkey(),
        &owner.pubkey(),
        rent,
    );
    let bh = rpc.get_latest_blockhash().unwrap();
    let tx =
        Transaction::new_signed_with_payer(&ixs, Some(&owner.pubkey()), &[owner, nonce_account], bh);
    rpc.send_and_confirm_transaction(&tx).unwrap();
}

fn init_vault(rpc: &RpcClient, program_id: &Pubkey, owner: &Keypair, beneficiary: &Pubkey, vault: &Pubkey) {
    let ix = heir_ix(
        program_id,
        &owner.pubkey(),
        beneficiary,
        vault,
        HeirInstruction::Initialize {
            beneficiary: *beneficiary,
            timeout_secs: TIMEOUT_SECS,
        },
    );
    let bh = rpc.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&owner.pubkey()), &[owner], bh);
    rpc.send_and_confirm_transaction(&tx).unwrap();
}

fn build_presigned_sweep(
    rpc: &RpcClient,
    program_id: &Pubkey,
    owner: &Keypair,
    beneficiary: &Pubkey,
    vault: &Pubkey,
    nonce_account: &Pubkey,
) -> Vec<u8> {
    let nonce_acc = rpc.get_account(nonce_account).unwrap();
    let durable_nonce_hash = decode_durable_nonce(&nonce_acc.data);

    let sweep_ix = heir_ix(program_id, &owner.pubkey(), beneficiary, vault, HeirInstruction::HeirSweep);
    let message = Message::new_with_nonce(vec![sweep_ix], Some(&owner.pubkey()), nonce_account, &owner.pubkey());
    let tx = Transaction::new(&[owner], message, durable_nonce_hash);
    bincode::serialize(&tx).unwrap()
}

/// Scenario A: the presigned tx is signed once, held untouched, and
/// submitted for the first time only after the timeout -- with the owner's
/// balance having changed in the meantime.
fn scenario_a_clean_claim(rpc: &RpcClient, program_id: &Pubkey) {
    println!("\n=== Scenario A: sign once, hold, claim after timeout (balance changes) ===");
    let owner = Keypair::new();
    let beneficiary = Keypair::new();
    let nonce_account = Keypair::new();
    let (vault, _bump) = vault_pda(&owner.pubkey(), program_id);

    airdrop(rpc, &owner.pubkey(), 2_000_000_000);
    create_nonce_account(rpc, &owner, &nonce_account);
    init_vault(rpc, program_id, &owner, &beneficiary.pubkey(), &vault);

    let presigned_bytes =
        build_presigned_sweep(rpc, program_id, &owner, &beneficiary.pubkey(), &vault, &nonce_account.pubkey());
    println!(
        "owner signed the durable-nonce heir_sweep tx once ({} bytes); \
         treating it as opaque, held bytes from here on",
        presigned_bytes.len()
    );

    println!(
        "waiting {}s past the vault timeout with NO submission attempts \
         (an impatient early attempt is covered in Scenario B)...",
        TIMEOUT_SECS + 2
    );
    // The owner's balance changes in the meantime -- ordinary activity,
    // unrelated to the inheritance plan, happening after the tx was signed.
    airdrop(rpc, &owner.pubkey(), 777_000_000);
    sleep(Duration::from_secs((TIMEOUT_SECS + 2) as u64));

    let live_balance = rpc.get_balance(&owner.pubkey()).unwrap();
    println!("owner's live balance right before claim: {live_balance} lamports (2_000_000_000 + 777_000_000 airdropped, minus fees)");

    let late_tx: Transaction = bincode::deserialize(&presigned_bytes).unwrap();
    match rpc.send_and_confirm_transaction(&late_tx) {
        Ok(sig) => println!("CLAIM SUCCEEDED: {sig}"),
        Err(e) => {
            println!("CLAIM FAILED (unexpected for this scenario): {e}");
            return;
        }
    }

    let beneficiary_balance = rpc.get_balance(&beneficiary.pubkey()).unwrap();
    let owner_balance_after = rpc.get_balance(&owner.pubkey()).unwrap();
    println!(
        "RESULT: beneficiary received {beneficiary_balance} lamports (the LIVE balance minus the claim tx's own fee); \
         owner's account now holds {owner_balance_after} lamports"
    );
    assert!(beneficiary_balance > 2_700_000_000, "swept amount should reflect the live, grown balance");
    assert_eq!(owner_balance_after, 0);
    println!("CONFIRMED: a durable-nonce presigned tx with no amount field, held for real elapsed time, \
        does sweep the LIVE balance at claim time -- the amount is not frozen at signing time.");
}

/// Scenario B: the critical flaw. A single premature submission attempt --
/// by anyone holding the bytes, not necessarily the beneficiary -- burns the
/// nonce even though the transaction fails, permanently destroying the only
/// valid copy of the owner's authorization. The owner is (by the premise of
/// this whole product) unavailable to sign a replacement.
fn scenario_b_premature_submission_is_fatal(rpc: &RpcClient, program_id: &Pubkey) {
    println!("\n=== Scenario B: one early submission attempt permanently kills the plan ===");
    let owner = Keypair::new();
    let beneficiary = Keypair::new();
    let nonce_account = Keypair::new();
    let (vault, _bump) = vault_pda(&owner.pubkey(), program_id);

    airdrop(rpc, &owner.pubkey(), 2_000_000_000);
    create_nonce_account(rpc, &owner, &nonce_account);
    init_vault(rpc, program_id, &owner, &beneficiary.pubkey(), &vault);

    let presigned_bytes =
        build_presigned_sweep(rpc, program_id, &owner, &beneficiary.pubkey(), &vault, &nonce_account.pubkey());

    let nonce_before = decode_durable_nonce(&rpc.get_account(&nonce_account.pubkey()).unwrap().data);
    println!("owner signed the durable-nonce heir_sweep tx once; nonce = {nonce_before}");

    println!(
        "someone (anyone with the bytes -- a curious beneficiary, or a malicious stranger) \
         submits it EARLY, with preflight simulation skipped (skip_preflight=true) -- e.g. a \
         relayer racing to be first, or just an impatient/hostile holder of the bytes..."
    );
    let early_tx: Transaction = bincode::deserialize(&presigned_bytes).unwrap();
    let (sig, result) = send_skip_preflight_and_wait(rpc, &early_tx);
    println!("early submission landed on-chain as {sig}, execution result: {result:?}");

    let nonce_after = decode_durable_nonce(&rpc.get_account(&nonce_account.pubkey()).unwrap().data);
    println!(
        "nonce after the FAILED early attempt = {nonce_after}  (changed: {})",
        nonce_after != nonce_before
    );

    println!(
        "waiting {}s past the vault timeout, then resubmitting the ORIGINAL signed bytes...",
        TIMEOUT_SECS + 2
    );
    sleep(Duration::from_secs((TIMEOUT_SECS + 2) as u64));

    let retry_tx: Transaction = bincode::deserialize(&presigned_bytes).unwrap();
    match rpc.send_and_confirm_transaction(&retry_tx) {
        Ok(sig) => println!("(unexpected: late resubmission of the same bytes succeeded: {sig})"),
        Err(e) => println!(
            "late resubmission of the SAME bytes FAILED, permanently: {e}\n\
             The owner is unavailable (that is the entire premise of this product) to sign a \
             replacement transaction against the now-advanced nonce. The inheritance plan is dead."
        ),
    }

    let beneficiary_balance = rpc.get_balance(&beneficiary.pubkey()).unwrap();
    println!("beneficiary balance: {beneficiary_balance} (should be 0 -- nothing was ever recoverable after the early attempt)");
    assert_eq!(beneficiary_balance, 0);
    assert_ne!(nonce_before, nonce_after, "expected the failed early attempt to still burn the nonce");
}

fn main() {
    let program_id_arg = std::env::args().nth(1).expect(
        "usage: cargo run --example durable_nonce_demo -- <PROGRAM_ID>\n\
         (the pubkey printed by `solana program deploy`, or read \
         target/deploy/heir_sweep_poc-keypair.json with `solana-keygen pubkey`)",
    );
    let program_id: Pubkey = program_id_arg.parse().expect("invalid program id");

    let rpc = RpcClient::new_with_commitment(RPC_URL.to_string(), CommitmentConfig::confirmed());

    scenario_a_clean_claim(&rpc, &program_id);
    scenario_b_premature_submission_is_fatal(&rpc, &program_id);
}
