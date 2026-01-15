#[cfg(test)]
mod tests {
    use litesvm::LiteSVM;
    use solana_sdk::{
        account::Account,
        clock::Clock,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };
    use pinocchio_system::ID;

    use spl_associated_token_account::ID as ATA_PROGRAM_ID;
    use spl_token::solana_program::program_option::COption;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::state::Mint;
    use spl_token::ID as TOKEN_PROGRAM_ID;

    const PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0f, 0x1e, 0x6b, 0x14, 0x21, 0xc0, 0x4a, 0x07, 0x04, 0x31, 0x26, 0x5c, 0x19, 0xc5, 0xbb,
        0xee, 0x19, 0x92, 0xba, 0xe8, 0xaf, 0xd1, 0xcd, 0x07, 0x8e, 0xf8, 0xaf, 0x70, 0x47, 0xdc,
        0x11, 0xf7,
    ]);

    // January 1, 2025 00:00:00 UTC
    const JAN_1_2025: i64 = 1735689600;
    const ONE_DAY: u64 = 86_400;

    // VestSchedule::LEN = discriminator(1) + 3*Pubkey(96) + 5*u64(40) + bump(1) = 138
    const VEST_SCHEDULE_LEN: usize = 138;

    fn create_initialize_instruction_data(
        seed: u64,
        start_timestamp: u64,
        cliff_duration: u64,
        total_duration: u64,
        step_duration: u64,
        bump: u8,
    ) -> Vec<u8> {
        let mut data = vec![0u8]; // Discriminator for Initialize
        data.extend_from_slice(&seed.to_le_bytes());
        data.extend_from_slice(&start_timestamp.to_le_bytes());
        data.extend_from_slice(&cliff_duration.to_le_bytes());
        data.extend_from_slice(&total_duration.to_le_bytes());
        data.extend_from_slice(&step_duration.to_le_bytes());
        data.push(bump);
        data
    }

    // Updated: PDA now uses only seed (no token_mint or initializer)
    fn derive_vest_schedule_pda(seed: u64) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[b"vest_schedule", &seed.to_le_bytes()],
            &PROGRAM_ID,
        )
    }

    fn derive_ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        spl_associated_token_account::get_associated_token_address(owner, mint)
    }

    fn setup_svm() -> LiteSVM {
        let mut svm = LiteSVM::new().with_builtins().with_sigverify(false);

        svm.add_program_from_file(PROGRAM_ID, "target/deploy/token_vesting.so")
            .expect("Failed to load program");

        // Warp to Jan 1, 2025
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

    fn build_initialize_instruction(
        initializer: &Pubkey,
        vest_schedule_pda: &Pubkey,
        token_mint: &Pubkey,
        vault: &Pubkey,
        instruction_data: Vec<u8>,
    ) -> Instruction {
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*initializer, true),
                AccountMeta::new(*vest_schedule_pda, false),
                AccountMeta::new_readonly(*token_mint, false),
                AccountMeta::new(*vault, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
                AccountMeta::new_readonly(ATA_PROGRAM_ID, false),
            ],
            data: instruction_data,
        }
    }

    #[test]
    fn test_initialize_success() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64; // Tomorrow
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_ok(), "Transaction should succeed");

        // Verify account was created
        let vest_schedule_account = svm.get_account(&vest_schedule_pda);
        assert!(vest_schedule_account.is_some(), "Vest schedule account should exist");

        let account = vest_schedule_account.unwrap();
        assert_eq!(account.owner, PROGRAM_ID, "Should be owned by program");
        assert_eq!(account.data.len(), VEST_SCHEDULE_LEN, "Should have correct data length");
    }

    #[test]
    fn test_initialize_start_timestamp_in_past() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 - ONE_DAY as i64) as u64; // Yesterday (in past!)
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with start timestamp in past");
    }

    #[test]
    fn test_initialize_cliff_greater_than_total() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;
        let cliff_duration = ONE_DAY * 10; // 10 days cliff
        let total_duration = ONE_DAY;      // 1 day total (less than cliff!)
        let step_duration = ONE_DAY;

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with cliff > total duration");
    }

    #[test]
    fn test_initialize_step_exceeds_total() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY * 20; // Step > total

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with step > total duration");
    }

    #[test]
    fn test_initialize_invalid_step_duration() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = 77777; // Doesn't divide evenly into (total - cliff)

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with invalid step duration");
    }

    #[test]
    fn test_initialize_zero_cliff_duration() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;
        let cliff_duration = 0; // Zero cliff
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with zero cliff duration");
    }

    #[test]
    fn test_initialize_zero_step_duration() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = 0; // Zero step

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with zero step duration");
    }

    #[test]
    fn test_initialize_zero_seed() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 0u64; // Zero seed - invalid
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with zero seed");
    }

    #[test]
    fn test_initialize_wrong_bump() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        let (vest_schedule_pda, correct_bump) = derive_vest_schedule_pda(seed);
        let wrong_bump = correct_bump.wrapping_add(1);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
            wrong_bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with wrong bump");
    }

    #[test]
    fn test_initialize_account_already_initialized() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data.clone(),
        );

        // First initialization
        let transaction = Transaction::new_signed_with_payer(
            &[instruction.clone()],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_ok(), "First initialization should succeed");

        // Second initialization should fail
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Second initialization should fail");
    }

    #[test]
    fn test_initialize_with_different_seeds() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 20_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;
        let seeds = [12345u64, 67890u64];

        for seed in seeds.iter() {
            let (vest_schedule_pda, bump) = derive_vest_schedule_pda(*seed);
            let vault = derive_ata(&vest_schedule_pda, &token_mint);

            let instruction_data = create_initialize_instruction_data(
                *seed,
                start_timestamp,
                ONE_DAY,
                ONE_DAY * 10,
                ONE_DAY,
                bump,
            );

            let instruction = build_initialize_instruction(
                &initializer.pubkey(),
                &vest_schedule_pda,
                &token_mint,
                &vault,
                instruction_data,
            );

            let transaction = Transaction::new_signed_with_payer(
                &[instruction],
                Some(&initializer.pubkey()),
                &[&initializer],
                svm.latest_blockhash(),
            );

            let result = svm.send_transaction(transaction);
            print_transaction_logs(&result);
            assert!(result.is_ok(), "Initialization with seed {} should succeed", seed);

            let account = svm.get_account(&vest_schedule_pda);
            assert!(account.is_some(), "Account for seed {} should exist", seed);
        }
    }

    #[test]
    fn test_initialize_insufficient_accounts() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let instruction_data = create_initialize_instruction_data(
            12345,
            (JAN_1_2025 + ONE_DAY as i64) as u64,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
            255,
        );

        // Only 3 accounts instead of 7
        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), true),
                AccountMeta::new(Pubkey::new_unique(), false),
                AccountMeta::new_readonly(Pubkey::new_unique(), false),
            ],
            data: instruction_data,
        };

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with insufficient accounts");
    }

    #[test]
    fn test_initialize_initializer_not_signer() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();
        let payer = Keypair::new();

        svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
            bump,
        );

        // Initializer NOT marked as signer
        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), false), // Not signer!
                AccountMeta::new(vest_schedule_pda, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
                AccountMeta::new_readonly(ATA_PROGRAM_ID, false),
            ],
            data: instruction_data,
        };

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[&payer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail when initializer is not signer");
    }

    #[test]
    fn test_initialize_cliff_equals_total() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;
        let cliff_duration = ONE_DAY * 10;
        let total_duration = ONE_DAY * 10; // Equal to cliff
        let step_duration = ONE_DAY;

        let (vest_schedule_pda, bump) = derive_vest_schedule_pda(seed);
        let vault = derive_ata(&vest_schedule_pda, &token_mint);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = build_initialize_instruction(
            &initializer.pubkey(),
            &vest_schedule_pda,
            &token_mint,
            &vault,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&initializer.pubkey()),
            &[&initializer],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail when cliff equals total duration");
    }
}