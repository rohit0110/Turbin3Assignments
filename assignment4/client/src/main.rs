//! Drives the deployed `stablecoin` program on devnet through all 6 tasks
//! with real transactions. Run with `cargo run --bin demo` after `anchor
//! deploy` has published the program under the id in `Anchor.toml`.

use std::path::PathBuf;

use anchor_lang::{
    solana_program::{instruction::Instruction, system_instruction, system_program},
    InstructionData, ToAccountMetas,
};
use anyhow::{Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_associated_token_account_interface::{
    address::get_associated_token_address_with_program_id,
    instruction::create_associated_token_account,
};

mod confidential;

pub(crate) fn token_program_id() -> Pubkey {
    spl_token_2022_interface::id()
}

fn rpc() -> RpcClient {
    RpcClient::new_with_commitment(
        "https://api.devnet.solana.com".to_string(),
        CommitmentConfig::confirmed(),
    )
}

// Same wallet the other assignments in this repo use on devnet.
fn load_payer() -> Result<Keypair> {
    let path = PathBuf::from(std::env::var("HOME")?).join(".config/solana/id.json");
    let bytes = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let raw: Vec<u8> = serde_json::from_str(&bytes)?;
    Ok(Keypair::try_from(raw.as_slice())?)
}

pub(crate) fn send(rpc: &RpcClient, ixs: &[Instruction], payer: &Pubkey, signers: &[&Keypair]) -> Result<()> {
    // the U256 range proof verify burns well past the default 200k CU budget
    let mut all_ixs = vec![solana_compute_budget_interface::ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)];
    all_ixs.extend_from_slice(ixs);

    let blockhash = rpc.get_latest_blockhash()?;
    let msg = Message::new_with_blockhash(&all_ixs, Some(payer), &blockhash);
    let tx = Transaction::new(signers, msg, blockhash);
    let sig = rpc.send_and_confirm_transaction(&tx)?;
    println!("  tx: {sig}");
    Ok(())
}

pub(crate) fn create_ata(rpc: &RpcClient, payer: &Keypair, owner: &Pubkey, mint: &Pubkey) -> Result<Pubkey> {
    let ix = create_associated_token_account(&payer.pubkey(), owner, mint, &token_program_id());
    send(rpc, &[ix], &payer.pubkey(), &[payer])?;
    Ok(get_associated_token_address_with_program_id(owner, mint, &token_program_id()))
}

pub(crate) fn thaw(rpc: &RpcClient, freeze_authority: &Keypair, token_account: &Pubkey, mint: &Pubkey) -> Result<()> {
    let ix = Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::ThawAccount {}.data(),
        stablecoin::accounts::ThawAccount {
            freeze_authority: freeze_authority.pubkey(),
            token_account: *token_account,
            mint: *mint,
            token_program: token_program_id(),
        }
        .to_account_metas(None),
    );
    send(rpc, &[ix], &freeze_authority.pubkey(), &[freeze_authority])
}

pub(crate) fn mint_to(rpc: &RpcClient, authority: &Keypair, mint: &Pubkey, dest: &Pubkey, amount: u64) -> Result<()> {
    let ix = spl_token_2022_interface::instruction::mint_to(
        &token_program_id(),
        mint,
        dest,
        &authority.pubkey(),
        &[],
        amount,
    )?;
    send(rpc, &[ix], &authority.pubkey(), &[authority])
}

#[allow(clippy::too_many_arguments)]
fn initialize_mint(
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
) -> Result<()> {
    let ix = Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::InitializeMint {
            decimals,
            mint_authority,
            freeze_authority,
            transfer_fee_config_authority,
            withdraw_withheld_authority,
            transfer_fee_basis_points,
            maximum_fee,
            close_authority,
        }
        .data(),
        stablecoin::accounts::InitializeMint {
            payer: payer.pubkey(),
            mint: mint.pubkey(),
            token_program: token_program_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    send(rpc, &[ix], &payer.pubkey(), &[payer, mint])
}

fn transfer_with_fee(
    rpc: &RpcClient,
    authority: &Keypair,
    source: &Pubkey,
    destination: &Pubkey,
    mint: &Pubkey,
    amount: u64,
) -> Result<()> {
    let ix = Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::TransferWithFee { amount }.data(),
        stablecoin::accounts::TransferWithFee {
            authority: authority.pubkey(),
            source: *source,
            destination: *destination,
            mint: *mint,
            token_program: token_program_id(),
        }
        .to_account_metas(None),
    );
    send(rpc, &[ix], &authority.pubkey(), &[authority])
}

pub(crate) fn token_balance(rpc: &RpcClient, ata: &Pubkey) -> Result<u64> {
    let data = rpc.get_account_data(ata)?;
    let state = spl_token_2022_interface::extension::StateWithExtensions::<
        spl_token_2022_interface::state::Account,
    >::unpack(&data)?;
    Ok(state.base.amount)
}

fn main() -> Result<()> {
    let rpc = rpc();
    let payer = load_payer()?;
    println!("payer / issuer: {}", payer.pubkey());
    println!(
        "payer balance: {} SOL",
        rpc.get_balance(&payer.pubkey())? as f64 / 1_000_000_000.0
    );

    let alice = Keypair::new();
    let bob = Keypair::new();
    // fund from the payer directly, no faucet rate limits this way
    send(
        &rpc,
        &[
            system_instruction::transfer(&payer.pubkey(), &alice.pubkey(), 50_000_000),
            system_instruction::transfer(&payer.pubkey(), &bob.pubkey(), 50_000_000),
        ],
        &payer.pubkey(),
        &[&payer],
    )?;

    // Task 1
    println!("\n=== Task 1: initialize_mint ===");
    let mint = Keypair::new();
    initialize_mint(
        &rpc,
        &payer,
        &mint,
        6,
        payer.pubkey(),
        payer.pubkey(),
        payer.pubkey(),
        payer.pubkey(),
        100,   // 1%
        1_000, // max fee, in base units
        payer.pubkey(),
    )?;
    println!("mint: {}", mint.pubkey());

    // Task 4
    println!("\n=== Task 4: thaw new (born-frozen) accounts ===");
    let alice_ata = create_ata(&rpc, &payer, &alice.pubkey(), &mint.pubkey())?;
    let bob_ata = create_ata(&rpc, &payer, &bob.pubkey(), &mint.pubkey())?;
    thaw(&rpc, &payer, &alice_ata, &mint.pubkey())?;
    thaw(&rpc, &payer, &bob_ata, &mint.pubkey())?;
    println!("alice_ata: {alice_ata}\nbob_ata: {bob_ata}");

    mint_to(&rpc, &payer, &mint.pubkey(), &alice_ata, 1_000_000)?;

    // Task 2 + 3
    println!("\n=== Task 2 + 3: transfer_with_fee ===");
    transfer_with_fee(&rpc, &alice, &alice_ata, &bob_ata, &mint.pubkey(), 100_000)?;
    println!("alice balance: {}", token_balance(&rpc, &alice_ata)?);
    println!("bob balance:   {} (100_000 minus the 1% fee, withheld on bob's account)", token_balance(&rpc, &bob_ata)?);

    // Task 5 + 6
    confidential::run(&rpc, &payer, &alice, &bob)?;

    Ok(())
}
