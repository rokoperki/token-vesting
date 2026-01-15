use pinocchio::{
    account_info::AccountInfo,
    instruction::{Seed, Signer},
    program_error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
};
use pinocchio_token::{instructions::Transfer, state::TokenAccount};

use crate::{
    AssociatedToken, PinocchioError, ProgramAccount, SignerAccount, VestParticipant, VestSchedule,
};

pub struct ClaimAccounts<'a> {
    pub participant: &'a AccountInfo,
    pub participant_state: &'a AccountInfo,
    pub participant_ata: &'a AccountInfo,
    pub vest_schedule: &'a AccountInfo,
    pub vault: &'a AccountInfo,
    pub token_mint: &'a AccountInfo,
    pub system_program: &'a AccountInfo,
    pub token_program: &'a AccountInfo,
    pub ata_program: &'a AccountInfo,
}

impl<'a> TryFrom<&'a [AccountInfo]> for ClaimAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        let [participant, participant_state, participant_ata, vest_schedule, vault, token_mint, system_program, token_program, ata_program] =
            accounts
        else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        SignerAccount::check(participant)?;
        ProgramAccount::check_token_program(token_program)?;
        ProgramAccount::check_system_program(system_program)?;
        ProgramAccount::check::<VestSchedule>(vest_schedule)?;
        ProgramAccount::check::<VestParticipant>(participant_state)?;
        ProgramAccount::check_ata_program(ata_program)?;

        Ok(Self {
            participant,
            participant_state,
            participant_ata,
            vest_schedule,
            vault,
            token_mint,
            system_program,
            token_program,
            ata_program,
        })
    }
}

pub struct Claim<'a> {
    pub accounts: ClaimAccounts<'a>,
}

impl<'a> TryFrom<&'a [AccountInfo]> for Claim<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        let accounts = ClaimAccounts::try_from(accounts)?;

        {
            let vest_schedule_data = accounts.vest_schedule.try_borrow_data()?;
            let vest_schedule = VestSchedule::load(&vest_schedule_data)?;

            let participant_state_data = accounts.participant_state.try_borrow_data()?;
            let participant_state = VestParticipant::load(&participant_state_data)?;

            if accounts.token_mint.key() != vest_schedule.token_mint() {
                return Err(ProgramError::InvalidAccountData);
            }

            AssociatedToken::check(
                accounts.vault,
                *accounts.vest_schedule.key(),
                *vest_schedule.token_mint(),
                *accounts.token_program.key(),
            )?;

            AssociatedToken::init_if_needed(
                accounts.participant_ata,
                accounts.token_mint,
                accounts.participant,
                accounts.participant,
                accounts.system_program,
                accounts.token_program,
            )?;

            ProgramAccount::verify(
                &[
                    Seed::from(b"vest_participant"),
                    Seed::from(accounts.participant.key().as_ref()),
                    Seed::from(accounts.vest_schedule.key().as_ref()),
                ],
                accounts.participant_state,
                participant_state.bump(),
            )?;

            if participant_state.participant() != accounts.participant.key() {
                return Err(ProgramError::IllegalOwner);
            }

            if participant_state.schedule() != accounts.vest_schedule.key() {
                return Err(ProgramError::InvalidAccountData);
            }
        }
        Ok(Self { accounts })
    }
}

impl<'a> Claim<'a> {
    pub const DISCRIMINATOR: &'a u8 = &2;

    pub fn process(&self) -> Result<(), ProgramError> {
        let (claimable_amount, allocated_amount, schedule_seed, schedule_bump) = {
            let vest_schedule_data = self.accounts.vest_schedule.try_borrow_data()?;
            let vest_schedule = VestSchedule::load(&vest_schedule_data)?;

            let participant_state_data = self.accounts.participant_state.try_borrow_data()?;
            let participant_state = VestParticipant::load(&participant_state_data)?;

            let current_timestamp = Clock::get()?.unix_timestamp as u64;
            let claimable_amount = vest_schedule.calculate_claimable_amount(
                current_timestamp,
                participant_state.allocated_amount(),
                participant_state.claimed_amount(),
            );

            (
                claimable_amount,
                participant_state.allocated_amount(),
                vest_schedule.seed(),
                vest_schedule.bump(),
            )
        }; // Both borrows dropped here

        if claimable_amount == 0 {
            return Err(PinocchioError::NoClaimableAmount.into());
        }

        {
            let vault_account = TokenAccount::from_account_info(self.accounts.vault)?;
            if vault_account.amount() < claimable_amount {
                return Err(ProgramError::InsufficientFunds);
            }
        }

        let seed_binding = schedule_seed.to_le_bytes();
        let bump_binding = [schedule_bump];
        let vest_schedule_seeds = [
            Seed::from(b"vest_schedule"),
            Seed::from(&seed_binding),
            Seed::from(&bump_binding),
        ];

        let signer = Signer::from(&vest_schedule_seeds);

        Transfer {
            from: self.accounts.vault,
            to: self.accounts.participant_ata,
            authority: self.accounts.vest_schedule,
            amount: claimable_amount,
        }
        .invoke_signed(&[signer])?;

        let mut participant_state_data = self.accounts.participant_state.try_borrow_mut_data()?;
        let participant_state = VestParticipant::load_mut(&mut participant_state_data)?;

        let new_claimed = participant_state
            .claimed_amount()
            .saturating_add(claimable_amount);

        if new_claimed > allocated_amount {
            return Err(PinocchioError::ClaimExceedsAllocation.into());
        }

        participant_state.set_claimed_amount(new_claimed);

        Ok(())
    }
}
