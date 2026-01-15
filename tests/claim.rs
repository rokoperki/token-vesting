#[cfg(test)]
mod claim_tests {
    use litesvm::LiteSVM;
    use solana_sdk::{
        account::Account,
        clock::Clock,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };
    use spl_token::solana_program::program_option::COption;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::state::{Account as TokenAccount, AccountState, Mint};
    use spl_token::ID as TOKEN_PROGRAM_ID;

    const PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0f, 0x1e, 0x6b, 0x14, 0x21, 0xc0, 0x4a, 0x07, 0x04, 0x31, 0x26, 0x5c, 0x19, 0xc5, 0xbb,
        0xee, 0x19, 0x92, 0xba, 0xe8, 0xaf, 0xd1, 0xcd, 0x07, 0x8e, 0xf8, 0xaf, 0x70, 0x47, 0xdc,
        0x11, 0xf7,
    ]);

    // January 1, 2025 00:00:00 UTC
    const JAN_1_2025: i64 = 1735689600;
    
    const ONE_DAY: u64 = 86_400;

    const CLAIM_DISCRIMINATOR: u8 = 2;

    fn create_claim_instruction_data() -> Vec<u8> {
        vec![CLAIM_DISCRIMINATOR]
    }

    fn derive_participant_pda(participant: &Pubkey, schedule: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[b"vest_participant", participant.as_ref(), schedule.as_ref()],
            &PROGRAM_ID,
        )
    }

    fn derive_vest_schedule_pda(
        seed: u64,
        token_mint: &Pubkey,
        initializer: &Pubkey,
    ) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[
                b"vest_schedule",
                &seed.to_le_bytes(),
                token_mint.as_ref(),
                initializer.as_ref(),
            ],
            &PROGRAM_ID,
        )
    }

    fn derive_ata(owner: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[owner.as_ref(), &TOKEN_PROGRAM_ID.to_bytes(), mint.as_ref()],
            &Pubkey::new_from_array(pinocchio_associated_token_account::ID),
        )
    }

    fn setup_svm() -> LiteSVM {
        let mut svm = LiteSVM::new().with_builtins().with_sigverify(false);
        svm.add_program_from_file(PROGRAM_ID, "target/deploy/token_vesting.so")
            .expect("Failed to load program");
        
        // Set clock to January 1, 2025
        warp_to_timestamp(&mut svm, JAN_1_2025);
        
        svm
    }

    fn warp_to_timestamp(svm: &mut LiteSVM, unix_timestamp: i64) {
        let current_clock = svm.get_sysvar::<Clock>();
        svm.set_sysvar(&Clock {
            unix_timestamp,
            ..current_clock
        });
    }

    fn print_transaction_logs(
        result: &Result<
            litesvm::types::TransactionMetadata,
            litesvm::types::FailedTransactionMetadata,
        >,
    ) {
        match result {
            Ok(meta) => {
                println!("\n=== Transaction Succeeded ===");
                for log in &meta.logs {
                    println!("  {}", log);
                }
            }
            Err(err) => {
                println!("\n=== Transaction Failed ===");
                println!("Error: {:?}", err.err);
                for log in &err.meta.logs {
                    println!("  {}", log);
                }
            }
        }
    }

    fn create_mock_token_mint(svm: &mut LiteSVM, authority: &Pubkey) -> Pubkey {
        let mint_keypair = Keypair::new();
        let mint_pubkey = mint_keypair.pubkey();

        let mint_data = Mint {
            mint_authority: COption::Some(*authority),
            supply: 1_000_000_000,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };

        let mut data = vec![0u8; Mint::LEN];
        Mint::pack(mint_data, &mut data).unwrap();

        svm.set_account(mint_pubkey, Account {
            lamports: 10_000_000,
            data,
            owner: TOKEN_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }.into());
        
        mint_pubkey
    }

    fn create_vest_schedule(
        svm: &mut LiteSVM,
        authority: &Pubkey,
        token_mint: &Pubkey,
        seed: u64,
        start_timestamp: u64,
        cliff_duration: u64,
        total_duration: u64,
        step_duration: u64,
    ) -> Pubkey {
        let (schedule_pda, bump) = derive_vest_schedule_pda(seed, token_mint, authority);

        let mut schedule_data = Vec::with_capacity(105);
        schedule_data.extend_from_slice(token_mint.as_ref());
        schedule_data.extend_from_slice(authority.as_ref());
        schedule_data.extend_from_slice(&seed.to_le_bytes());
        schedule_data.extend_from_slice(&start_timestamp.to_le_bytes());
        schedule_data.extend_from_slice(&cliff_duration.to_le_bytes());
        schedule_data.extend_from_slice(&total_duration.to_le_bytes());
        schedule_data.extend_from_slice(&step_duration.to_le_bytes());
        schedule_data.push(bump);

        svm.set_account(schedule_pda, Account {
            lamports: 10_000_000,
            data: schedule_data,
            owner: PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }.into());
        
        schedule_pda
    }

    fn create_participant_state(
        svm: &mut LiteSVM,
        participant: &Pubkey,
        schedule: &Pubkey,
        allocated_amount: u64,
        claimed_amount: u64,
    ) -> Pubkey {
        let (participant_state, bump) = derive_participant_pda(participant, schedule);

        let mut data = Vec::with_capacity(81);
        data.extend_from_slice(participant.as_ref());
        data.extend_from_slice(schedule.as_ref());
        data.extend_from_slice(&allocated_amount.to_le_bytes());
        data.extend_from_slice(&claimed_amount.to_le_bytes());
        data.push(bump);

        svm.set_account(participant_state, Account {
            lamports: 10_000_000,
            data,
            owner: PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }.into());
        
        participant_state
    }

    fn create_ata_with_balance(
        svm: &mut LiteSVM,
        owner: &Pubkey,
        mint: &Pubkey,
        amount: u64,
    ) -> Pubkey {
        let (ata, _) = derive_ata(owner, mint);

        let token_account = TokenAccount {
            mint: *mint,
            owner: *owner,
            amount,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };

        let mut data = vec![0u8; TokenAccount::LEN];
        TokenAccount::pack(token_account, &mut data).unwrap();

        svm.set_account(ata, Account {
            lamports: 10_000_000,
            data,
            owner: TOKEN_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }.into());
        
        ata
    }

    fn build_claim_instruction(
        participant: &Pubkey,
        participant_state: &Pubkey,
        participant_ata: &Pubkey,
        vest_schedule: &Pubkey,
        vault: &Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*participant, true),
                AccountMeta::new(*participant_state, false),
                AccountMeta::new(*participant_ata, false),
                AccountMeta::new(*vest_schedule, false), // Must be mutable - program borrows mutably
                AccountMeta::new(*vault, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
            data: create_claim_instruction_data(),
        }
    }

    // ==================== CRITICAL EDGE CASES ====================

    /// Claim at first claimable moment (cliff end + 1 step)
    #[test]
    fn test_claim_exactly_at_cliff_end() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        // Schedule: started Dec 30, 2024, cliff = 1 day, step = 1 day
        // Cliff ends Dec 31, first step completes Jan 1 at 00:00:00
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 2) as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            1,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
        );

        let allocated = 1_000_000u64;
        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule,
            allocated,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, allocated);
        let participant_ata = create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_ok(), "Should succeed at first claimable moment");
    }

    /// Claim 1 second before cliff ends (should fail)
    #[test]
    fn test_claim_one_second_before_cliff() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        // Cliff ends 1 second after current time
        let start_timestamp = (JAN_1_2025 - ONE_DAY as i64 + 1) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            2,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
        );

        let allocated = 1_000_000u64;
        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule,
            allocated,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, allocated);
        let participant_ata = create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail 1 second before cliff");
    }

    /// Claim exactly at cliff end but 0 steps elapsed (should fail - nothing claimable yet)
    #[test]
    fn test_claim_at_cliff_end_zero_steps() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        // Cliff ends exactly at Jan 1, 2025 but no steps have completed
        let start_timestamp = (JAN_1_2025 - ONE_DAY as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            20,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
        );

        let allocated = 1_000_000u64;
        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule,
            allocated,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, allocated);
        let participant_ata = create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail at cliff end with 0 steps elapsed");
    }

    /// Claim when already claimed everything vested so far
    #[test]
    fn test_claim_nothing_new_to_claim() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        // Started 2 days ago, cliff 1 day, 10 day total, 1 day steps
        // At day 2: 1 step completed = 1/9 vested ≈ 111,111
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 2) as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;
        let allocated = 1_000_000u64;
        
        // Already claimed the 1 step worth
        let already_claimed = allocated / 9;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            3,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
        );

        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule,
            allocated,
            already_claimed,
        );

        let vault = create_ata_with_balance(
            &mut svm,
            &participant_state,
            &token_mint,
            allocated - already_claimed,
        );
        let participant_ata = create_ata_with_balance(
            &mut svm,
            &participant.pubkey(),
            &token_mint,
            already_claimed,
        );

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail when nothing new to claim");
    }

    /// Vault has less tokens than claimable (drained vault)
    #[test]
    fn test_claim_vault_drained() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        // Fully vested schedule
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 30) as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            4,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
        );

        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule,
            allocated,
            0,
        );

        // Vault only has 1 token
        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 1);
        let participant_ata = create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with insufficient vault balance");
    }

    /// Wrong participant tries to claim
    #[test]
    fn test_claim_wrong_signer() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let real_participant = Keypair::new();
        let attacker = Keypair::new();
        svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 5) as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            5,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
        );

        let participant_state = create_participant_state(
            &mut svm,
            &real_participant.pubkey(),
            &schedule,
            allocated,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, allocated);
        let participant_ata = create_ata_with_balance(&mut svm, &real_participant.pubkey(), &token_mint, 0);

        // Attacker signs
        let instruction = build_claim_instruction(
            &attacker.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&attacker.pubkey()),
            &[&attacker],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with wrong signer");
    }

    /// Mismatched schedule
    #[test]
    fn test_claim_schedule_mismatch() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 5) as i64) as u64;

        let schedule_1 = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            100,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let schedule_2 = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            200,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        // Linked to schedule_1
        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule_1,
            1_000_000,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 1_000_000);
        let participant_ata = create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        // Use schedule_2
        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule_2,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with schedule mismatch");
    }

    /// Wrong destination ATA
    #[test]
    fn test_claim_wrong_destination_ata() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        let attacker = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 5) as i64) as u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            6,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule,
            1_000_000,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 1_000_000);
        let attacker_ata = create_ata_with_balance(&mut svm, &attacker.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &attacker_ata,
            &schedule,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with wrong destination ATA");
    }

    /// Double claim in same TX
    #[test]
    fn test_double_claim_same_tx() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 15) as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            7,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule,
            allocated,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, allocated);
        let participant_ata = create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
        );

        // Two claims in one TX
        let tx = Transaction::new_signed_with_payer(
            &[instruction.clone(), instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Double claim in same TX should fail");
    }

    /// Zero allocation
    #[test]
    fn test_claim_zero_allocation() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 5) as i64) as u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            8,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule,
            0,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 0);
        let participant_ata = create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with zero allocation");
    }

    /// Success: fully vested claim
    #[test]
    fn test_claim_success_fully_vested() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 30) as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            9,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule,
            allocated,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, allocated);
        let participant_ata = create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_ok(), "Fully vested claim should succeed");

        // Verify transfer
        let ata_account = svm.get_account(&participant_ata).unwrap();
        let token_data = TokenAccount::unpack(&ata_account.data).unwrap();
        assert_eq!(token_data.amount, allocated);
    }
}