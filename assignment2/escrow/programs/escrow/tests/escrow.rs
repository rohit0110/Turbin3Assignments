//! End-to-end tests for the escrow program, driven with LiteSVM.
//!
//! Run `anchor build` first so that `target/deploy/escrow.so` exists, then
//! `cargo test`. LiteSVM ships the SPL Token + Associated Token programs, so
//! we can mint real tokens and move them around here.

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::{
            get_associated_token_address, spl_associated_token_account, ID as ATA_PROGRAM_ID,
        },
        token::spl_token,
    },
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const ESCROW_SO: &[u8] =
    include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/escrow.so"));

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
// SPL Mint account size, i.e. spl_token::state::Mint::LEN.
const MINT_LEN: usize = 82;

// --- little helpers -------------------------------------------------------

fn send(svm: &mut LiteSVM, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    svm.send_transaction(tx).unwrap();
}

// Spin up a mint with `payer` as both the rent payer and the mint authority.
fn create_mint(svm: &mut LiteSVM, payer: &Keypair, decimals: u8) -> Pubkey {
    let mint = Keypair::new();
    let rent = svm.minimum_balance_for_rent_exemption(MINT_LEN);
    let create = system_instruction::create_account(
        &payer.pubkey(),
        &mint.pubkey(),
        rent,
        MINT_LEN as u64,
        &spl_token::ID,
    );
    let init = spl_token::instruction::initialize_mint2(
        &spl_token::ID,
        &mint.pubkey(),
        &payer.pubkey(),
        None,
        decimals,
    )
    .unwrap();
    send(svm, &[create, init], payer, &[payer, &mint]);
    mint.pubkey()
}

fn create_ata(svm: &mut LiteSVM, payer: &Keypair, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    let ix = spl_associated_token_account::instruction::create_associated_token_account(
        &payer.pubkey(),
        owner,
        mint,
        &spl_token::ID,
    );
    send(svm, &[ix], payer, &[payer]);
    get_associated_token_address(owner, mint)
}

fn mint_tokens(svm: &mut LiteSVM, authority: &Keypair, mint: &Pubkey, dest: &Pubkey, amount: u64) {
    let ix = spl_token::instruction::mint_to(
        &spl_token::ID,
        mint,
        dest,
        &authority.pubkey(),
        &[],
        amount,
    )
    .unwrap();
    send(svm, &[ix], authority, &[authority]);
}

fn token_balance(svm: &LiteSVM, ata: &Pubkey) -> u64 {
    let acc = svm.get_account(ata).expect("token account should exist");
    anchor_spl::token::TokenAccount::try_deserialize(&mut &acc.data[..])
        .unwrap()
        .amount
}

fn escrow_pda(maker: &Pubkey, seed: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[
            escrow::constants::ESCROW_SEED,
            maker.as_ref(),
            &seed.to_le_bytes(),
        ],
        &escrow::id(),
    )
    .0
}

// --- shared fixture -----------------------------------------------------

struct Env {
    svm: LiteSVM,
    maker: Keypair,
    taker: Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
    maker_ata_a: Pubkey,
    taker_ata_b: Pubkey,
}

// Maker starts with 1_000_000 of mint_a, taker starts with 1_000_000 of mint_b.
fn setup() -> Env {
    let mut svm = LiteSVM::new();
    svm.add_program(escrow::id(), ESCROW_SO).unwrap();

    let maker = Keypair::new();
    let taker = Keypair::new();
    let mint_auth = Keypair::new();
    for kp in [&maker, &taker, &mint_auth] {
        svm.airdrop(&kp.pubkey(), 100 * LAMPORTS_PER_SOL).unwrap();
    }

    let mint_a = create_mint(&mut svm, &mint_auth, 6);
    let mint_b = create_mint(&mut svm, &mint_auth, 6);

    let maker_ata_a = create_ata(&mut svm, &maker, &maker.pubkey(), &mint_a);
    let taker_ata_b = create_ata(&mut svm, &taker, &taker.pubkey(), &mint_b);
    mint_tokens(&mut svm, &mint_auth, &mint_a, &maker_ata_a, 1_000_000);
    mint_tokens(&mut svm, &mint_auth, &mint_b, &taker_ata_b, 1_000_000);

    Env {
        svm,
        maker,
        taker,
        mint_a,
        mint_b,
        maker_ata_a,
        taker_ata_b,
    }
}

// --- instruction builders --------------------------------------------------

fn make_ix(env: &Env, seed: u64, deposit: u64, receive: u64) -> Instruction {
    let escrow = escrow_pda(&env.maker.pubkey(), seed);
    let vault = get_associated_token_address(&escrow, &env.mint_a);
    Instruction::new_with_bytes(
        escrow::id(),
        &escrow::instruction::Make {
            seed,
            deposit,
            receive,
        }
        .data(),
        escrow::accounts::Make {
            maker: env.maker.pubkey(),
            mint_a: env.mint_a,
            mint_b: env.mint_b,
            maker_ata_a: env.maker_ata_a,
            escrow,
            vault,
            associated_token_program: ATA_PROGRAM_ID,
            token_program: spl_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn update_ix(env: &Env, seed: u64, receive: u64) -> Instruction {
    let escrow = escrow_pda(&env.maker.pubkey(), seed);
    Instruction::new_with_bytes(
        escrow::id(),
        &escrow::instruction::Update { receive }.data(),
        escrow::accounts::Update {
            maker: env.maker.pubkey(),
            escrow,
        }
        .to_account_metas(None),
    )
}

fn take_ix(env: &Env, seed: u64) -> Instruction {
    let escrow = escrow_pda(&env.maker.pubkey(), seed);
    let vault = get_associated_token_address(&escrow, &env.mint_a);
    Instruction::new_with_bytes(
        escrow::id(),
        &escrow::instruction::Take {}.data(),
        escrow::accounts::Take {
            taker: env.taker.pubkey(),
            maker: env.maker.pubkey(),
            mint_a: env.mint_a,
            mint_b: env.mint_b,
            taker_ata_a: get_associated_token_address(&env.taker.pubkey(), &env.mint_a),
            taker_ata_b: env.taker_ata_b,
            maker_ata_b: get_associated_token_address(&env.maker.pubkey(), &env.mint_b),
            escrow,
            vault,
            associated_token_program: ATA_PROGRAM_ID,
            token_program: spl_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn refund_ix(env: &Env, seed: u64) -> Instruction {
    let escrow = escrow_pda(&env.maker.pubkey(), seed);
    let vault = get_associated_token_address(&escrow, &env.mint_a);
    Instruction::new_with_bytes(
        escrow::id(),
        &escrow::instruction::Refund {}.data(),
        escrow::accounts::Refund {
            maker: env.maker.pubkey(),
            mint_a: env.mint_a,
            maker_ata_a: env.maker_ata_a,
            escrow,
            vault,
            token_program: spl_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn read_escrow(env: &Env, seed: u64) -> escrow::state::Escrow {
    let acc = env.svm.get_account(&escrow_pda(&env.maker.pubkey(), seed)).unwrap();
    escrow::state::Escrow::try_deserialize(&mut &acc.data[..]).unwrap()
}

// --- tests --------------------------------------------------------------

#[test]
fn make_locks_the_deposit_and_records_the_terms() {
    let mut env = setup();
    let seed = 1;

    let ix = make_ix(&env, seed, 1_000_000, 500_000);
    send(&mut env.svm, &[ix], &env.maker, &[&env.maker]);

    let vault = get_associated_token_address(&escrow_pda(&env.maker.pubkey(), seed), &env.mint_a);
    assert_eq!(token_balance(&env.svm, &vault), 1_000_000);
    assert_eq!(token_balance(&env.svm, &env.maker_ata_a), 0);

    let state = read_escrow(&env, seed);
    assert_eq!(state.seed, seed);
    assert_eq!(state.maker, env.maker.pubkey());
    assert_eq!(state.mint_a, env.mint_a);
    assert_eq!(state.mint_b, env.mint_b);
    assert_eq!(state.receive, 500_000);
}

#[test]
fn update_rewrites_the_receive_amount() {
    let mut env = setup();
    let seed = 2;

    let make = make_ix(&env, seed, 1_000_000, 500_000);
    send(&mut env.svm, &[make], &env.maker, &[&env.maker]);
    let update = update_ix(&env, seed, 250_000);
    send(&mut env.svm, &[update], &env.maker, &[&env.maker]);

    assert_eq!(read_escrow(&env, seed).receive, 250_000);
}

#[test]
fn take_completes_the_swap_and_closes_everything() {
    let mut env = setup();
    let seed = 3;

    let make = make_ix(&env, seed, 1_000_000, 400_000);
    send(&mut env.svm, &[make], &env.maker, &[&env.maker]);
    let take = take_ix(&env, seed);
    send(&mut env.svm, &[take], &env.taker, &[&env.taker]);

    let taker_ata_a = get_associated_token_address(&env.taker.pubkey(), &env.mint_a);
    let maker_ata_b = get_associated_token_address(&env.maker.pubkey(), &env.mint_b);

    // Taker walked away with all of mint_a...
    assert_eq!(token_balance(&env.svm, &taker_ata_a), 1_000_000);
    // ...and paid the maker the agreed 400_000 of mint_b.
    assert_eq!(token_balance(&env.svm, &maker_ata_b), 400_000);
    assert_eq!(token_balance(&env.svm, &env.taker_ata_b), 600_000);

    // Escrow record and its vault are both gone.
    let escrow_addr = escrow_pda(&env.maker.pubkey(), seed);
    let vault = get_associated_token_address(&escrow_addr, &env.mint_a);
    assert!(env.svm.get_account(&escrow_addr).is_none());
    assert!(env.svm.get_account(&vault).is_none());
}

#[test]
fn refund_gives_the_maker_their_tokens_back() {
    let mut env = setup();
    let seed = 4;

    let make = make_ix(&env, seed, 1_000_000, 500_000);
    send(&mut env.svm, &[make], &env.maker, &[&env.maker]);
    let refund = refund_ix(&env, seed);
    send(&mut env.svm, &[refund], &env.maker, &[&env.maker]);

    assert_eq!(token_balance(&env.svm, &env.maker_ata_a), 1_000_000);

    let escrow_addr = escrow_pda(&env.maker.pubkey(), seed);
    let vault = get_associated_token_address(&escrow_addr, &env.mint_a);
    assert!(env.svm.get_account(&escrow_addr).is_none());
    assert!(env.svm.get_account(&vault).is_none());
}
