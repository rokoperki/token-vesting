#[cfg(test)]
mod claim_tests {
    use litesvm::LiteSVM;
    use pinocchio_system::ID;
    use solana_sdk::{
        account::Account,
        clock::Clock,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };
    use spl_associated_token_account::ID as ATA_PROGRAM_ID;
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

    // VestSchedule::LEN = discriminator(1) + 3*Pubkey(96) + 5*u64(40) + bump(1) = 138
    const VEST_SCHEDULE_LEN: usize = 138;
    // VestParticipant::LEN = discriminator(1) + 2*Pubkey(64) + 2*u64(16) + bump(1) = 82
    const VEST_PARTICIPANT_LEN: usize = 82;

    fn create_claim_instruction_data() -> Vec<u8> {
        vec![CLAIM_DISCRIMINATOR]
    }

    fn derive_participant_pda(participant: &Pubkey, schedule: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[b"vest_participant", participant.as_ref(), schedule.as_ref()],
            &PROGRAM_ID,
        )
    }

    // Updated: PDA now uses only seed
    fn derive_vest_schedule_pda(seed: u64) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"vest_schedule", &seed.to_le_bytes()], &PROGRAM_ID)
    }

    fn derive_ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        spl_associated_token_account::get_associated_token_address(owner, mint)
    }

    fn setup_svm() -> LiteSVM {
        let mut svm = LiteSVM::new().with_builtins().with_sigverify(false);
        svm.add_program_from_file(PROGRAM_ID, "target/deploy/token_vesting.so")
            .expect("Failed to load program");

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

        svm.set_account(
            mint_pubkey,
            Account {
                lamports: 10_000_000,
                data,
                owner: TOKEN_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            }
            .into(),
        );

        mint_pubkey
    }

    // Updated: VestSchedule now has discriminator and vault field (138 bytes)
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
        let (schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&schedule_pda, token_mint);

        let mut schedule_data = Vec::with_capacity(VEST_SCHEDULE_LEN);
        schedule_data.push(0u8); // Discriminator
        schedule_data.extend_from_slice(token_mint.as_ref()); // Token mint: Pubkey (32)
        schedule_data.extend_from_slice(authority.as_ref()); // Authority: Pubkey (32)
        schedule_data.extend_from_slice(vault.as_ref()); // Vault: Pubkey (32)
        schedule_data.extend_from_slice(&seed.to_le_bytes()); // Seed: u64 (8)
        schedule_data.extend_from_slice(&start_timestamp.to_le_bytes()); // Start timestamp: u64 (8)
        schedule_data.extend_from_slice(&cliff_duration.to_le_bytes()); // Cliff duration: u64 (8)
        schedule_data.extend_from_slice(&total_duration.to_le_bytes()); // Total duration: u64 (8)
        schedule_data.extend_from_slice(&step_duration.to_le_bytes()); // Step duration: u64 (8)
        schedule_data.push(bump); // Bump: u8 (1)

        assert_eq!(schedule_data.len(), VEST_SCHEDULE_LEN);

        svm.set_account(
            schedule_pda,
            Account {
                lamports: 10_000_000,
                data: schedule_data,
                owner: PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            }
            .into(),
        );

        schedule_pda
    }

    // Updated: VestParticipant now has discriminator (82 bytes)
    fn create_participant_state(
        svm: &mut LiteSVM,
        participant: &Pubkey,
        schedule: &Pubkey,
        allocated_amount: u64,
        claimed_amount: u64,
    ) -> Pubkey {
        let (participant_state, bump) = derive_participant_pda(participant, schedule);

        let mut data = Vec::with_capacity(VEST_PARTICIPANT_LEN);
        data.push(1u8); // Discriminator
        data.extend_from_slice(participant.as_ref()); // Participant: Pubkey (32)
        data.extend_from_slice(schedule.as_ref()); // Schedule: Pubkey (32)
        data.extend_from_slice(&allocated_amount.to_le_bytes()); // Allocated: u64 (8)
        data.extend_from_slice(&claimed_amount.to_le_bytes()); // Claimed: u64 (8)
        data.push(bump); // Bump: u8 (1)

        assert_eq!(data.len(), VEST_PARTICIPANT_LEN);

        svm.set_account(
            participant_state,
            Account {
                lamports: 10_000_000,
                data,
                owner: PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            }
            .into(),
        );

        participant_state
    }

    fn create_ata_with_balance(
        svm: &mut LiteSVM,
        owner: &Pubkey,
        mint: &Pubkey,
        amount: u64,
    ) -> Pubkey {
        let ata = derive_ata(owner, mint);

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

        svm.set_account(
            ata,
            Account {
                lamports: 10_000_000,
                data,
                owner: TOKEN_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            }
            .into(),
        );

        ata
    }

    // Updated: 9 accounts now
    fn build_claim_instruction(
        participant: &Pubkey,
        participant_state: &Pubkey,
        participant_ata: &Pubkey,
        vest_schedule: &Pubkey,
        vault: &Pubkey,
        token_mint: &Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*participant, true),
                AccountMeta::new(*participant_state, false),
                AccountMeta::new(*participant_ata, false),
                AccountMeta::new(*vest_schedule, false),
                AccountMeta::new(*vault, false),
                AccountMeta::new_readonly(*token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
                AccountMeta::new_readonly(ATA_PROGRAM_ID, false),
            ],
            data: create_claim_instruction_data(),
        }
    }

    // ==================== SUCCESS CASES ====================

    #[test]
    fn test_claim_success_fully_vested() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        // Started 30 days ago, fully vested
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 30) as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            1,
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

        // Vault owned by schedule
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
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

        // Verify vault is empty
        let vault_account = svm.get_account(&vault).unwrap();
        let vault_data = TokenAccount::unpack(&vault_account.data).unwrap();
        assert_eq!(vault_data.amount, 0);
    }

    #[test]
    fn test_claim_partial_vesting() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        // Started 3 days ago, cliff 1 day, 10 day total, 1 day steps
        // 2 steps completed = 2/9 vested
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 3) as i64) as u64;
        let allocated = 900_000u64; // Divisible by 9

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            2,
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

        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_ok(), "Partial claim should succeed");

        // 2/9 of 900,000 = 200,000
        let expected_claim = 200_000u64;
        let ata_account = svm.get_account(&participant_ata).unwrap();
        let token_data = TokenAccount::unpack(&ata_account.data).unwrap();
        assert_eq!(token_data.amount, expected_claim);
    }

    #[test]
    fn test_claim_exactly_at_cliff_end() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        // Started Dec 30, cliff ends Dec 31, first step completes Jan 1
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 2) as i64) as u64;
        let allocated = 900_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            3,
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

        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
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

    #[test]
    fn test_claim_multiple_times() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        // Started 3 days ago
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 3) as i64) as u64;
        let allocated = 900_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            4,
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

        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        // First claim
        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_ok(), "First claim should succeed");

        // Warp forward 2 more days
        warp_to_timestamp(&mut svm, JAN_1_2025 + (ONE_DAY * 2) as i64);

        // Second claim
        let instruction2 = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
        );

        let tx2 = Transaction::new_signed_with_payer(
            &[instruction2],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result2 = svm.send_transaction(tx2);
        print_transaction_logs(&result2);
        assert!(result2.is_ok(), "Second claim should succeed");

        // Should have claimed 4/9 total now
        let expected_total = 400_000u64;
        let ata_account = svm.get_account(&participant_ata).unwrap();
        let token_data = TokenAccount::unpack(&ata_account.data).unwrap();
        assert_eq!(token_data.amount, expected_total);
    }

    // ==================== FAILURE CASES ====================

    #[test]
    fn test_claim_before_cliff() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        // Cliff ends 1 second after current time
        let start_timestamp = (JAN_1_2025 - ONE_DAY as i64 + 1) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            10,
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

        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail before cliff");
    }

    #[test]
    fn test_claim_at_cliff_end_zero_steps() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        // Cliff ends exactly now but no steps completed
        let start_timestamp = (JAN_1_2025 - ONE_DAY as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            11,
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

        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail at cliff end with 0 steps");
    }

    #[test]
    fn test_claim_nothing_new_to_claim() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        // Started 2 days ago, 1 step completed = 1/9 vested
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 2) as i64) as u64;
        let allocated = 900_000u64;
        let already_claimed = 100_000u64; // Already claimed the 1 step

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            12,
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
            already_claimed,
        );

        let vault = create_ata_with_balance(
            &mut svm,
            &schedule,
            &token_mint,
            allocated - already_claimed,
        );
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, already_claimed);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
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

    #[test]
    fn test_claim_vault_insufficient_balance() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        // Fully vested
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 30) as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            13,
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

        // Vault only has 1 token
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 1);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
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

    #[test]
    fn test_claim_wrong_signer() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let real_participant = Keypair::new();
        let attacker = Keypair::new();
        svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 5) as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            14,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let participant_state = create_participant_state(
            &mut svm,
            &real_participant.pubkey(),
            &schedule,
            allocated,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &real_participant.pubkey(), &token_mint, 0);

        // Attacker tries to claim
        let instruction = build_claim_instruction(
            &attacker.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
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

    #[test]
    fn test_claim_wrong_schedule() {
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

        // Participant linked to schedule_1
        let participant_state = create_participant_state(
            &mut svm,
            &participant.pubkey(),
            &schedule_1,
            1_000_000,
            0,
        );

        let vault = create_ata_with_balance(&mut svm, &schedule_1, &token_mint, 1_000_000);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        // Try to use schedule_2
        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule_2,
            &vault,
            &token_mint,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with wrong schedule");
    }

    #[test]
    fn test_claim_wrong_token_mint() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let wrong_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 5) as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            15,
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

        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        // Pass wrong mint
        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &wrong_mint,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with wrong token mint");
    }

    #[test]
    fn test_claim_participant_not_signer() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 5) as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            16,
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

        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        // Participant NOT marked as signer
        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(participant.pubkey(), false), // Not signer!
                AccountMeta::new(participant_state, false),
                AccountMeta::new(participant_ata, false),
                AccountMeta::new(schedule, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
                AccountMeta::new_readonly(ATA_PROGRAM_ID, false),
            ],
            data: create_claim_instruction_data(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[&payer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail when participant not signer");
    }

    #[test]
    fn test_claim_double_claim_same_tx() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        // Fully vested
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 15) as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            17,
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

        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &vault,
            &token_mint,
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

    #[test]
    fn test_claim_wrong_vault() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();
        let random_owner = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());

        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 5) as i64) as u64;
        let allocated = 1_000_000u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            18,
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

        // Wrong vault (owned by random, not schedule)
        let wrong_vault =
            create_ata_with_balance(&mut svm, &random_owner.pubkey(), &token_mint, allocated);
        let participant_ata =
            create_ata_with_balance(&mut svm, &participant.pubkey(), &token_mint, 0);

        let instruction = build_claim_instruction(
            &participant.pubkey(),
            &participant_state,
            &participant_ata,
            &schedule,
            &wrong_vault,
            &token_mint,
        );

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with wrong vault");
    }

    #[test]
    fn test_claim_insufficient_accounts() {
        let mut svm = setup_svm();

        let participant = Keypair::new();
        svm.airdrop(&participant.pubkey(), 10_000_000_000).unwrap();

        // Only 5 accounts instead of 9
        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(participant.pubkey(), true),
                AccountMeta::new(Pubkey::new_unique(), false),
                AccountMeta::new(Pubkey::new_unique(), false),
                AccountMeta::new(Pubkey::new_unique(), false),
                AccountMeta::new(Pubkey::new_unique(), false),
            ],
            data: create_claim_instruction_data(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&participant.pubkey()),
            &[&participant],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(tx);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with insufficient accounts");
    }
}