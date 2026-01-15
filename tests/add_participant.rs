#[cfg(test)]
mod tests {
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

    // VestSchedule::LEN = discriminator(1) + 3*Pubkey(96) + 5*u64(40) + bump(1) = 138
    const VEST_SCHEDULE_LEN: usize = 138;

    fn create_add_participant_instruction_data(
        allocated_amount: u64,
        participant_bump: u8,
    ) -> Vec<u8> {
        let mut data = vec![1u8]; // Discriminator for AddParticipant
        data.extend_from_slice(&allocated_amount.to_le_bytes());
        data.push(participant_bump);
        data
    }

    fn derive_participant_pda(participant: &Pubkey, schedule: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[b"vest_participant", participant.as_ref(), schedule.as_ref()],
            &PROGRAM_ID,
        )
    }

    // Updated: PDA now uses only seed
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

        // VestSchedule: 138 bytes
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

        svm.set_account(schedule_pda, Account {
            lamports: 10_000_000,
            data: schedule_data,
            owner: PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }.into());

        schedule_pda
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

        svm.set_account(ata, Account {
            lamports: 10_000_000,
            data,
            owner: TOKEN_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }.into());

        ata
    }

    fn build_add_participant_instruction(
        authority: &Pubkey,
        authority_ata: &Pubkey,
        vault: &Pubkey,
        participant: &Pubkey,
        participant_state: &Pubkey,
        schedule: &Pubkey,
        token_mint: &Pubkey,
        instruction_data: Vec<u8>,
    ) -> Instruction {
        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*authority, true),
                AccountMeta::new(*authority_ata, false),
                AccountMeta::new(*vault, false),
                AccountMeta::new_readonly(*participant, false),
                AccountMeta::new(*participant_state, false),
                AccountMeta::new(*schedule, false),
                AccountMeta::new_readonly(*token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
            data: instruction_data,
        }
    }

    #[test]
    fn test_add_participant_success() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        // Schedule starts tomorrow (before cliff)
        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
        );

        let authority_ata = create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) = derive_participant_pda(&participant.pubkey(), &schedule);

        // Vault is owned by schedule, not participant_state
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = build_add_participant_instruction(
            &authority.pubkey(),
            &authority_ata,
            &vault,
            &participant.pubkey(),
            &participant_state,
            &schedule,
            &token_mint,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_ok(), "Transaction should succeed");

        // Verify participant state was created
        let participant_account = svm.get_account(&participant_state);
        assert!(participant_account.is_some(), "Participant state should exist");

        let account = participant_account.unwrap();
        assert_eq!(account.owner, PROGRAM_ID, "Should be owned by program");

        // Verify token transfer happened
        let vault_account = svm.get_account(&vault).unwrap();
        let vault_token_account = TokenAccount::unpack(&vault_account.data).unwrap();
        assert_eq!(vault_token_account.amount, allocated_amount, "Vault should have allocated amount");

        let authority_ata_account = svm.get_account(&authority_ata).unwrap();
        let authority_token_account = TokenAccount::unpack(&authority_ata_account.data).unwrap();
        assert_eq!(
            authority_token_account.amount,
            1_000_000 - allocated_amount,
            "Authority ATA should have reduced balance"
        );
    }

    #[test]
    fn test_add_participant_after_cliff() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        // Schedule started 5 days ago, cliff is 1 day (already passed!)
        let start_timestamp = (JAN_1_2025 - (ONE_DAY * 5) as i64) as u64;
        let cliff_duration = ONE_DAY;
        let total_duration = ONE_DAY * 10;
        let step_duration = ONE_DAY;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
        );

        let authority_ata = create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) = derive_participant_pda(&participant.pubkey(), &schedule);
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = build_add_participant_instruction(
            &authority.pubkey(),
            &authority_ata,
            &vault,
            &participant.pubkey(),
            &participant_state,
            &schedule,
            &token_mint,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail - cannot add participants after cliff");
    }

    #[test]
    fn test_add_participant_insufficient_funds() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        // Only 50k tokens
        let authority_ata = create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 50_000);

        let (participant_state, participant_bump) = derive_participant_pda(&participant.pubkey(), &schedule);
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 0);

        // Trying to allocate 100k
        let allocated_amount = 100_000u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = build_add_participant_instruction(
            &authority.pubkey(),
            &authority_ata,
            &vault,
            &participant.pubkey(),
            &participant_state,
            &schedule,
            &token_mint,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with insufficient funds");
    }

    #[test]
    fn test_add_participant_wrong_authority() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let wrong_authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&wrong_authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        // Schedule created with `authority`
        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        // But we use wrong_authority
        let wrong_authority_ata = create_ata_with_balance(&mut svm, &wrong_authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) = derive_participant_pda(&participant.pubkey(), &schedule);
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = build_add_participant_instruction(
            &wrong_authority.pubkey(),
            &wrong_authority_ata,
            &vault,
            &participant.pubkey(),
            &participant_state,
            &schedule,
            &token_mint,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&wrong_authority.pubkey()),
            &[&wrong_authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with wrong authority");
    }

    #[test]
    fn test_add_participant_zero_allocation() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let authority_ata = create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) = derive_participant_pda(&participant.pubkey(), &schedule);
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 0);

        // Zero allocation
        let allocated_amount = 0u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = build_add_participant_instruction(
            &authority.pubkey(),
            &authority_ata,
            &vault,
            &participant.pubkey(),
            &participant_state,
            &schedule,
            &token_mint,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with zero allocation");
    }

    #[test]
    fn test_add_participant_wrong_token_mint() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let wrong_token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        // Schedule created with token_mint
        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        // But we pass wrong_token_mint
        let authority_ata = create_ata_with_balance(&mut svm, &authority.pubkey(), &wrong_token_mint, 1_000_000);

        let (participant_state, participant_bump) = derive_participant_pda(&participant.pubkey(), &schedule);
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = build_add_participant_instruction(
            &authority.pubkey(),
            &authority_ata,
            &vault,
            &participant.pubkey(),
            &participant_state,
            &schedule,
            &wrong_token_mint,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with wrong token mint");
    }

    #[test]
    fn test_add_participant_vault_not_created() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let authority_ata = create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) = derive_participant_pda(&participant.pubkey(), &schedule);

        // Derive vault but DON'T create it
        let vault = derive_ata(&schedule, &token_mint);

        let allocated_amount = 100_000u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = build_add_participant_instruction(
            &authority.pubkey(),
            &authority_ata,
            &vault,
            &participant.pubkey(),
            &participant_state,
            &schedule,
            &token_mint,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail when vault is not created");
    }

    #[test]
    fn test_add_participant_double_initialization() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let authority_ata = create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) = derive_participant_pda(&participant.pubkey(), &schedule);
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = build_add_participant_instruction(
            &authority.pubkey(),
            &authority_ata,
            &vault,
            &participant.pubkey(),
            &participant_state,
            &schedule,
            &token_mint,
            instruction_data,
        );

        // First transaction should succeed
        let transaction = Transaction::new_signed_with_payer(
            &[instruction.clone()],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_ok(), "First transaction should succeed");

        // Second transaction should fail
        let transaction2 = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result2 = svm.send_transaction(transaction2);
        print_transaction_logs(&result2);
        assert!(result2.is_err(), "Should prevent double initialization");
    }

    #[test]
    fn test_add_participant_wrong_bump() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let authority_ata = create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, correct_bump) = derive_participant_pda(&participant.pubkey(), &schedule);
        let wrong_bump = correct_bump.wrapping_add(1);
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, wrong_bump);

        let instruction = build_add_participant_instruction(
            &authority.pubkey(),
            &authority_ata,
            &vault,
            &participant.pubkey(),
            &participant_state,
            &schedule,
            &token_mint,
            instruction_data,
        );

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_err(), "Should fail with wrong bump");
    }

    #[test]
    fn test_add_participant_authority_not_signer() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let payer = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let start_timestamp = (JAN_1_2025 + ONE_DAY as i64) as u64;

        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            start_timestamp,
            ONE_DAY,
            ONE_DAY * 10,
            ONE_DAY,
        );

        let authority_ata = create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) = derive_participant_pda(&participant.pubkey(), &schedule);
        let vault = create_ata_with_balance(&mut svm, &schedule, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data = create_add_participant_instruction_data(allocated_amount, participant_bump);

        // Authority NOT marked as signer
        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), false), // Not signer!
                AccountMeta::new(authority_ata, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(participant.pubkey(), false),
                AccountMeta::new(participant_state, false),
                AccountMeta::new(schedule, false),
                AccountMeta::new_readonly(token_mint, false),
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
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
        assert!(result.is_err(), "Should fail when authority is not signer");
    }
}