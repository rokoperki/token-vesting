use pinocchio::{
    account_info::AccountInfo,
    instruction::Seed,
    program_error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
};
use pinocchio_token::{instructions::Transfer, state::TokenAccount};

use crate::{
    participant_state, AssociatedToken, Mint, PinocchioError, ProgramAccount, SignerAccount,
    VestParticipant, VestSchedule,
};

pub struct AddParticipantAccounts<'a> {
    pub authority: &'a AccountInfo,
    pub authority_ata: &'a AccountInfo,
    pub vault: &'a AccountInfo,
    pub participant: &'a AccountInfo,
    pub participant_state: &'a AccountInfo,
    pub schedule: &'a AccountInfo,
    pub token_mint: &'a AccountInfo,
    pub system_program: &'a AccountInfo,
    pub token_program: &'a AccountInfo,
}

impl<'a> TryFrom<&'a [AccountInfo]> for AddParticipantAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        let [authority, authority_ata, vault, participant, participant_state, schedule, token_mint, system_program, token_program] =
            accounts
        else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        SignerAccount::check(authority)?;
        ProgramAccount::check_system_program(system_program)?;
        ProgramAccount::check_token_program(token_program)?;
        ProgramAccount::check_schedule(schedule)?;
        Mint::check(token_mint)?;

        Ok(Self {
            authority,
            authority_ata,
            vault,
            participant,
            participant_state,
            schedule,
            token_mint,
            system_program,
            token_program,
        })
    }
}

#[repr(C, packed)]
pub struct AddParticipantInstructionData {
    pub allocated_amount: u64,
    pub participant_bump: u8,
}

impl TryFrom<&[u8]> for AddParticipantInstructionData {
    type Error = ProgramError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        if data.len() != size_of::<u64>() + size_of::<u8>() {
            return Err(ProgramError::InvalidInstructionData);
        }

        let allocated_amount = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let participant_bump = u8::from_le_bytes(data[8..9].try_into().unwrap());

        if allocated_amount == 0 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(Self {
            allocated_amount,
            participant_bump,
        })
    }
}

pub struct AddParticipant<'a> {
    pub accounts: AddParticipantAccounts<'a>,
    pub instruction_data: AddParticipantInstructionData,
}

impl<'a> TryFrom<(&[u8], &'a [AccountInfo])> for AddParticipant<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&[u8], &'a [AccountInfo])) -> Result<Self, Self::Error> {
        let accounts = AddParticipantAccounts::try_from(accounts)?;
        let instruction_data = AddParticipantInstructionData::try_from(data)?;

        let vest_schedule_data = accounts.schedule.try_borrow_data()?;
        let vest_schedule = VestSchedule::load(&vest_schedule_data)?;

        let current_timestamp = Clock::get()?.unix_timestamp as u64;

        if vest_schedule.is_cliff_completed(current_timestamp) {
            return Err(PinocchioError::CannotAddParticipantsAfterCliff.into());
        }

        if accounts.authority.key() != vest_schedule.authority() {
            return Err(ProgramError::IllegalOwner);
        }

        if *accounts.token_mint.key() != *vest_schedule.token_mint() {
            return Err(ProgramError::InvalidAccountData);
        }

        ProgramAccount::verify(
            &[
                Seed::from(b"vest_participant"),
                Seed::from(accounts.participant.key().as_ref()),
                Seed::from(accounts.schedule.key().as_ref()),
            ],
            accounts.participant_state,
            instruction_data.participant_bump,
        )?;

        AssociatedToken::check(
            accounts.authority_ata,
            *accounts.authority.key(),
            *accounts.token_mint.key(),
            *accounts.token_program.key(),
        )?;

        let authority_ata = TokenAccount::from_account_info(accounts.authority_ata)?;
        if authority_ata.amount() < instruction_data.allocated_amount {
            return Err(ProgramError::InsufficientFunds);
        }

        AssociatedToken::check(
            accounts.vault,
            *accounts.participant_state.key(),
            *accounts.token_mint.key(),
            *accounts.token_program.key(),
        )?;

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> AddParticipant<'a> {
    pub const DISCRIMINATOR: &'a u8 = &1;

    pub fn process(&self) -> Result<(), ProgramError> {
        let bump_binding = [self.instruction_data.participant_bump];
        let participant_seeds = [
            Seed::from(b"vest_participant"),
            Seed::from(self.accounts.participant.key().as_ref()),
            Seed::from(self.accounts.schedule.key().as_ref()),
            Seed::from(&bump_binding),
        ];

        ProgramAccount::init::<VestParticipant>(
            self.accounts.authority,
            self.accounts.participant_state,
            &participant_seeds,
            participant_state::VestParticipant::LEN,
        )?;

        let mut participant_state_data = self.accounts.participant_state.try_borrow_mut_data()?;

        let participant_state = VestParticipant::load_mut(&mut participant_state_data)?;

        participant_state.set_inner(
            *self.accounts.participant.key(),
            *self.accounts.schedule.key(),
            self.instruction_data.allocated_amount,
            0,
            self.instruction_data.participant_bump,
        );

        Transfer {
            from: self.accounts.authority_ata,
            to: self.accounts.vault,
            authority: self.accounts.authority,
            amount: self.instruction_data.allocated_amount,
        }
        .invoke()?;
        Ok(())
    }
}
