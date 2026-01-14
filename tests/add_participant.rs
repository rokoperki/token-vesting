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
    use spl_token::state::{Account as TokenAccount, AccountState, Mint};
    use spl_token::ID as TOKEN_PROGRAM_ID;

    const PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0f, 0x1e, 0x6b, 0x14, 0x21, 0xc0, 0x4a, 0x07, 0x04, 0x31, 0x26, 0x5c, 0x19, 0xc5, 0xbb,
        0xee, 0x19, 0x92, 0xba, 0xe8, 0xaf, 0xd1, 0xcd, 0x07, 0x8e, 0xf8, 0xaf, 0x70, 0x47, 0xdc,
        0x11, 0xf7,
    ]);

    // Helper to create instruction data for AddParticipant
    fn create_add_participant_instruction_data(
        allocated_amount: u64,
        participant_bump: u8,
    ) -> Vec<u8> {
        let mut data = vec![1u8]; // Discriminator for AddParticipant
        data.extend_from_slice(&allocated_amount.to_le_bytes());
        data.push(participant_bump);
        data
    }

    // Helper to derive participant PDA
    fn derive_participant_pda(participant: &Pubkey, schedule: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[b"vest_participant", participant.as_ref(), schedule.as_ref()],
            &PROGRAM_ID,
        )
    }

    // Helper to derive vest schedule PDA
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

    // Helper to derive ATA
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

    fn create_vest_schedule(
        svm: &mut LiteSVM,
        authority: &Pubkey,
        token_mint: &Pubkey,
        seed: u64,
        status: u8,
    ) -> Pubkey {
        let (schedule_pda, bump) = derive_vest_schedule_pda(seed, token_mint, authority);

        let mut schedule_data = Vec::new();
        schedule_data.push(status); // Status: u8
        schedule_data.extend_from_slice(token_mint.as_ref()); // Token mint: Pubkey
        schedule_data.extend_from_slice(authority.as_ref()); // Authority: Pubkey
        schedule_data.extend_from_slice(&seed.to_le_bytes()); // Seed: u64
        schedule_data.extend_from_slice(&1000000000u64.to_le_bytes()); // Start timestamp: u64
        schedule_data.extend_from_slice(&86400u64.to_le_bytes()); // Cliff duration: u64
        schedule_data.extend_from_slice(&864000u64.to_le_bytes()); // Total duration: u64
        schedule_data.extend_from_slice(&86400u64.to_le_bytes()); // Step duration: u64
        schedule_data.push(bump); // Bump: u8

        assert_eq!(schedule_data.len(), 106);

        let schedule_account = Account {
            lamports: 10_000_000,
            data: schedule_data,
            owner: PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        };

        svm.set_account(schedule_pda, schedule_account.into());
        schedule_pda
    }

    fn create_ata_with_balance(
        svm: &mut LiteSVM,
        owner: &Pubkey,
        mint: &Pubkey,
        amount: u64,
    ) -> Pubkey {
        let (ata, _) = derive_ata(owner, mint);

        // Create proper SPL Token Account using Pack
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

        let ata_account = Account {
            lamports: 10_000_000,
            data,
            owner: TOKEN_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        };

        svm.set_account(ata, ata_account.into());
        ata
    }

    #[test]
    fn test_add_participant_success() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        // Create a vest schedule in NotStarted status (0)
        let schedule = create_vest_schedule(&mut svm, &authority.pubkey(), &token_mint, seed, 0);

        // Create authority's ATA with balance
        let authority_ata =
            create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) =
            derive_participant_pda(&participant.pubkey(), &schedule);

        // Pre-create the vault ATA (required since init_if_needed was removed)
        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data =
            create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
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
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(result.is_ok(), "Transaction should succeed");

        // Verify participant state was created
        let participant_account = svm.get_account(&participant_state);
        assert!(
            participant_account.is_some(),
            "Participant state should exist"
        );

        let account = participant_account.unwrap();
        assert_eq!(account.owner, PROGRAM_ID, "Should be owned by program");

        // Verify token transfer happened
        let vault_account = svm.get_account(&vault).unwrap();
        let vault_token_account = TokenAccount::unpack(&vault_account.data).unwrap();
        assert_eq!(
            vault_token_account.amount, allocated_amount,
            "Vault should have allocated amount"
        );

        let authority_ata_account = svm.get_account(&authority_ata).unwrap();
        let authority_token_account = TokenAccount::unpack(&authority_ata_account.data).unwrap();
        assert_eq!(
            authority_token_account.amount,
            1_000_000 - allocated_amount,
            "Authority ATA should have reduced balance"
        );
    }

    #[test]
    fn test_add_participant_insufficient_funds() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let schedule = create_vest_schedule(&mut svm, &authority.pubkey(), &token_mint, seed, 0);

        // Create authority's ATA with insufficient balance
        let authority_ata = create_ata_with_balance(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            50_000, // Only 50k tokens
        );

        let (participant_state, participant_bump) =
            derive_participant_pda(&participant.pubkey(), &schedule);

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 0);

        let allocated_amount = 100_000u64; // Trying to allocate 100k (more than available)
        let instruction_data =
            create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
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
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(
            result.is_err(),
            "Transaction should fail with insufficient funds"
        );
    }

    #[test]
    fn test_add_participant_wrong_authority() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let wrong_authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();
        svm.airdrop(&wrong_authority.pubkey(), 10_000_000_000)
            .unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        // Schedule created with `authority`
        let schedule = create_vest_schedule(&mut svm, &authority.pubkey(), &token_mint, seed, 0);

        // But we use wrong_authority's ATA
        let wrong_authority_ata =
            create_ata_with_balance(&mut svm, &wrong_authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) =
            derive_participant_pda(&participant.pubkey(), &schedule);

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data =
            create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(wrong_authority.pubkey(), true), // Wrong authority!
                AccountMeta::new(wrong_authority_ata, false),
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
            Some(&wrong_authority.pubkey()),
            &[&wrong_authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(
            result.is_err(),
            "Transaction should fail with wrong authority"
        );
    }

    #[test]
    fn test_add_participant_invalid_schedule_status() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        // Create schedule in Vesting status (2) - not allowed for adding participants
        let schedule = create_vest_schedule(
            &mut svm,
            &authority.pubkey(),
            &token_mint,
            seed,
            2, // Vesting status
        );

        let authority_ata =
            create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) =
            derive_participant_pda(&participant.pubkey(), &schedule);

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data =
            create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
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
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(
            result.is_err(),
            "Transaction should fail with invalid schedule status"
        );
    }

    #[test]
    fn test_add_participant_zero_allocation() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let schedule = create_vest_schedule(&mut svm, &authority.pubkey(), &token_mint, seed, 0);

        let authority_ata =
            create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) =
            derive_participant_pda(&participant.pubkey(), &schedule);

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 0);

        let allocated_amount = 0u64; // Zero allocation
        let instruction_data =
            create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
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
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(
            result.is_err(),
            "Transaction should fail with zero allocation"
        );
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

        // Schedule created with token_mint
        let schedule = create_vest_schedule(&mut svm, &authority.pubkey(), &token_mint, seed, 0);

        // But we use wrong_token_mint for ATA
        let authority_ata =
            create_ata_with_balance(&mut svm, &authority.pubkey(), &wrong_token_mint, 1_000_000);

        let (participant_state, participant_bump) =
            derive_participant_pda(&participant.pubkey(), &schedule);

        let vault = create_ata_with_balance(&mut svm, &participant_state, &token_mint, 0);

        let allocated_amount = 100_000u64;
        let instruction_data =
            create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(authority_ata, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(participant.pubkey(), false),
                AccountMeta::new(participant_state, false),
                AccountMeta::new(schedule, false),
                AccountMeta::new_readonly(wrong_token_mint, false), // Wrong mint!
                AccountMeta::new_readonly(ID.into(), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
            data: instruction_data,
        };

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(
            result.is_err(),
            "Transaction should fail with wrong token mint"
        );
    }

    #[test]
    fn test_add_participant_vault_not_created() {
        let mut svm = setup_svm();

        let authority = Keypair::new();
        let participant = Keypair::new();

        svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

        let token_mint = create_mock_token_mint(&mut svm, &authority.pubkey());
        let seed = 12345u64;

        let schedule = create_vest_schedule(&mut svm, &authority.pubkey(), &token_mint, seed, 0);

        let authority_ata =
            create_ata_with_balance(&mut svm, &authority.pubkey(), &token_mint, 1_000_000);

        let (participant_state, participant_bump) =
            derive_participant_pda(&participant.pubkey(), &schedule);

        // Derive vault address but DON'T create it
        let (vault, _) = derive_ata(&participant_state, &token_mint);

        let allocated_amount = 100_000u64;
        let instruction_data =
            create_add_participant_instruction_data(allocated_amount, participant_bump);

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
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
            Some(&authority.pubkey()),
            &[&authority],
            svm.latest_blockhash(),
        );

        let result = svm.send_transaction(transaction);
        print_transaction_logs(&result);
        assert!(
            result.is_err(),
            "Transaction should fail when vault is not pre-created"
        );
    }
}
