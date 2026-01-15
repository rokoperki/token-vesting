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
    pub token_program: &'a AccountInfo,
}

impl<'a> TryFrom<&'a [AccountInfo]> for ClaimAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        let [participant, participant_state, participant_ata, vest_schedule, vault, token_program] =
            accounts
        else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        SignerAccount::check(participant)?;
        ProgramAccount::check_token_program(token_program)?;
        ProgramAccount::check_schedule(vest_schedule)?;
        ProgramAccount::check_participant(participant_state)?;

        Ok(Self {
            participant,
            participant_state,
            participant_ata,
            vest_schedule,
            vault,
            token_program,
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

            AssociatedToken::check(
                accounts.vault,
                *accounts.participant_state.key(),
                *vest_schedule.token_mint(),
                *accounts.token_program.key(),
            )?;

            AssociatedToken::check(
                accounts.participant_ata,
                *accounts.participant.key(),
                *vest_schedule.token_mint(),
                *accounts.token_program.key(),
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
        let (claimable_amount, allocated_amount, bump) = {
            let participant_state_data = self.accounts.participant_state.try_borrow_data()?;
            let participant_state = VestParticipant::load(&participant_state_data)?;

            let vest_schedule_data = self.accounts.vest_schedule.try_borrow_data()?;
            let vest_schedule = VestSchedule::load(&vest_schedule_data)?;

            let current_timestamp = Clock::get()?.unix_timestamp as u64;
            let claimable_amount = vest_schedule.calculate_claimable_amount(
                current_timestamp,
                participant_state.allocated_amount(),
                participant_state.claimed_amount(),
            );

            (
                claimable_amount,
                participant_state.allocated_amount(),
                participant_state.bump(),
            )
        };

        if claimable_amount == 0 {
            return Err(PinocchioError::NoClaimableAmount.into());
        }

        {
            let vault_account = TokenAccount::from_account_info(self.accounts.vault)?;
            if vault_account.amount() < claimable_amount {
                return Err(ProgramError::InsufficientFunds);
            }
        }

        let binding = bump.to_le_bytes();
        let participant_seeds = [
            Seed::from(b"vest_participant"),
            Seed::from(self.accounts.participant.key().as_ref()),
            Seed::from(self.accounts.vest_schedule.key().as_ref()),
            Seed::from(&binding),
        ];

        let signer = Signer::from(&participant_seeds);

        Transfer {
            from: self.accounts.vault,
            // nebi bilo lose imat provjeru jeli useru otvorena ata za taj mint i ako nije mozda je i otvoriti
            to: self.accounts.participant_ata,
            authority: self.accounts.participant_state,
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
