#[cfg(test)]
mod tests {
    use litesvm::LiteSVM;
    use pinocchio_system::ID;
    use solana_sdk::{
        account::Account,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };
    use spl_token::solana_program::program_option::COption;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::state::Mint;
    use spl_token::ID as TOKEN_PROGRAM_ID;

    const PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0f, 0x1e, 0x6b, 0x14, 0x21, 0xc0, 0x4a, 0x07, 0x04, 0x31, 0x26, 0x5c, 0x19, 0xc5, 0xbb,
        0xee, 0x19, 0x92, 0xba, 0xe8, 0xaf, 0xd1, 0xcd, 0x07, 0x8e, 0xf8, 0xaf, 0x70, 0x47, 0xdc,
        0x11, 0xf7,
    ]);

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

    fn setup_svm() -> LiteSVM {
        let mut svm = LiteSVM::new().with_builtins().with_sigverify(false);

        svm.add_program_from_file(PROGRAM_ID, "target/deploy/token_vesting.so")
            .expect("Failed to load program");

        svm
    }

    fn print_transaction_logs(
        result: &Result<
            litesvm::types::TransactionMetadata,
            litesvm::types::FailedTransactionMetadata,
        >,
    ) {
        if let Err(err) = result {
            println!("\n=== Transaction Failed ===");
            println!("Error: {:?}", err.err);
            println!("\nProgram Logs:");
            for log in &err.meta.logs {
                println!("  {}", log);
            }
            println!(
                "Compute units consumed: {}",
                err.meta.compute_units_consumed
            );
            println!("========================\n");
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

        let mint_account = Account {
            lamports: 10_000_000,
            data,
            owner: TOKEN_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        };

        svm.set_account(mint_pubkey, mint_account.into());
        mint_pubkey
    }

    #[test]
    fn test_initialize_success() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 3600; // 1 hour in future
        let cliff_duration = 86400; // 1 day
        let total_duration = 864000; // 10 days
        let step_duration = 86400; // 1 day

        let (vest_schedule_pda, bump) =
            derive_vest_schedule_pda(seed, &token_mint, &initializer.pubkey());

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), true),
                AccountMeta::new(vest_schedule_pda, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
        assert!(result.is_ok(), "Transaction should succeed");

        // Verify account was created
        let vest_schedule_account = svm.get_account(&vest_schedule_pda);
        assert!(
            vest_schedule_account.is_some(),
            "Vest schedule account should exist"
        );

        let account = vest_schedule_account.unwrap();
        assert_eq!(account.owner, PROGRAM_ID, "Should be owned by program");
        assert!(account.lamports > 0, "Should have lamports for rent");

        // Verify account has correct data length (removed status field)
        // VestSchedule::LEN = 2 * Pubkey (64) + 5 * u64 (40) + 1 * u8 (1) = 105 bytes
        assert_eq!(account.data.len(), 105, "Should have correct data length");
    }

    #[test]
    fn test_initialize_cliff_greater_than_total() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 7200; // 2 hours in future
        let cliff_duration = 864000; // 10 days
        let total_duration = 86400; // 1 day (less than cliff)
        let step_duration = 86400;

        let (vest_schedule_pda, bump) =
            derive_vest_schedule_pda(seed, &token_mint, &initializer.pubkey());

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), true),
                AccountMeta::new(vest_schedule_pda, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
        assert!(result.is_err(), "Should fail with cliff > total duration");
    }

    #[test]
    fn test_initialize_step_exceeds_total() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 7200;
        let cliff_duration = 86400;
        let total_duration = 864000;
        let step_duration = 1000000; // Step > total

        let (vest_schedule_pda, bump) =
            derive_vest_schedule_pda(seed, &token_mint, &initializer.pubkey());

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), true),
                AccountMeta::new(vest_schedule_pda, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
        assert!(result.is_err(), "Should fail with step > total duration");
    }

    #[test]
    fn test_initialize_invalid_step_duration() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 7200;
        let cliff_duration = 86400;
        let total_duration = 864000; // 10 days
        let step_duration = 77777; // Doesn't divide evenly

        let (vest_schedule_pda, bump) =
            derive_vest_schedule_pda(seed, &token_mint, &initializer.pubkey());

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), true),
                AccountMeta::new(vest_schedule_pda, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
        assert!(result.is_err(), "Should fail with invalid step duration");
    }

    #[test]
    fn test_initialize_zero_cliff_duration() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 7200;
        let cliff_duration = 0; // Zero cliff
        let total_duration = 864000;
        let step_duration = 86400;

        let (vest_schedule_pda, bump) =
            derive_vest_schedule_pda(seed, &token_mint, &initializer.pubkey());

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), true),
                AccountMeta::new(vest_schedule_pda, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
        assert!(result.is_err(), "Should fail with zero cliff duration");
    }

    #[test]
    fn test_initialize_zero_step_duration() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 7200;
        let cliff_duration = 86400;
        let total_duration = 864000;
        let step_duration = 0; // Zero step

        let (vest_schedule_pda, bump) =
            derive_vest_schedule_pda(seed, &token_mint, &initializer.pubkey());

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        );

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), true),
                AccountMeta::new(vest_schedule_pda, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
        assert!(result.is_err(), "Should fail with zero step duration");
    }

    #[test]
    fn test_initialize_wrong_bump() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 7200;

        let (vest_schedule_pda, correct_bump) =
            derive_vest_schedule_pda(seed, &token_mint, &initializer.pubkey());

        let wrong_bump = correct_bump.wrapping_add(1);

        let instruction_data = create_initialize_instruction_data(
            seed,
            start_timestamp,
            86400,
            864000,
            86400,
            wrong_bump,
        );

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), true),
                AccountMeta::new(vest_schedule_pda, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
        assert!(result.is_err(), "Should fail with wrong bump");
    }

    #[test]
    fn test_initialize_insufficient_accounts() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let instruction_data =
            create_initialize_instruction_data(12345, 1000000, 86400, 864000, 86400, 255);

        // Only provide 3 accounts instead of 5
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
    fn test_initialize_account_already_initialized() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &initializer.pubkey());

        let seed = 12345u64;
        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 7200;

        let (vest_schedule_pda, bump) =
            derive_vest_schedule_pda(seed, &token_mint, &initializer.pubkey());

        let instruction_data =
            create_initialize_instruction_data(seed, start_timestamp, 86400, 864000, 86400, bump);

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(initializer.pubkey(), true),
                AccountMeta::new(vest_schedule_pda, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
            data: instruction_data.clone(),
        };

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

        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 7200;

        let seeds = [12345u64, 67890u64];

        for seed in seeds.iter() {
            let (vest_schedule_pda, bump) =
                derive_vest_schedule_pda(*seed, &token_mint, &initializer.pubkey());

            let instruction_data = create_initialize_instruction_data(
                *seed,
                start_timestamp,
                86400,
                864000,
                86400,
                bump,
            );

            let instruction = Instruction {
                program_id: PROGRAM_ID,
                accounts: vec![
                    AccountMeta::new(initializer.pubkey(), true),
                    AccountMeta::new(vest_schedule_pda, false),
                    AccountMeta::new_readonly(token_mint, false),
                    AccountMeta::new_readonly(ID.into(), false),
                    AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
            assert!(
                result.is_ok(),
                "Initialization with seed {} should succeed",
                seed
            );

            let account = svm.get_account(&vest_schedule_pda);
            assert!(
                account.is_some(),
                "Account for seed {} should exist",
                seed
            );
        }
    }

    #[test]
    fn test_initialize_with_different_mints() {
        let mut svm = setup_svm();
        let initializer = Keypair::new();

        svm.airdrop(&initializer.pubkey(), 20_000_000_000).unwrap();

        let current_time = svm
            .get_sysvar::<solana_sdk::clock::Clock>()
            .unix_timestamp as u64;
        let start_timestamp = current_time + 7200;
        let seed = 12345u64;

        let token_mint_1 = create_mock_token_mint(&mut svm, &initializer.pubkey());
        let token_mint_2 = create_mock_token_mint(&mut svm, &initializer.pubkey());

        for token_mint in [token_mint_1, token_mint_2].iter() {
            let (vest_schedule_pda, bump) =
                derive_vest_schedule_pda(seed, token_mint, &initializer.pubkey());

            let instruction_data = create_initialize_instruction_data(
                seed,
                start_timestamp,
                86400,
                864000,
                86400,
                bump,
            );

            let instruction = Instruction {
                program_id: PROGRAM_ID,
                accounts: vec![
                    AccountMeta::new(initializer.pubkey(), true),
                    AccountMeta::new(vest_schedule_pda, false),
                    AccountMeta::new_readonly(*token_mint, false),
                    AccountMeta::new_readonly(ID.into(), false),
                    AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
            assert!(
                result.is_ok(),
                "Initialization with mint {:?} should succeed",
                token_mint
            );

            let account = svm.get_account(&vest_schedule_pda);
            assert!(
                account.is_some(),
                "Account for mint {:?} should exist",
                token_mint
            );
        }
    }
}