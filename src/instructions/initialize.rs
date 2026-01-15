use pinocchio::{
    account_info::AccountInfo,
    instruction::Seed,
    program_error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
    ProgramResult,
};

use crate::{Mint, PinocchioError, ProgramAccount, SignerAccount, VestSchedule};

pub struct InitializeAccounts<'a> {
    pub initializer: &'a AccountInfo,
    pub vest_schedule: &'a AccountInfo,
    pub token_mint: &'a AccountInfo,
    // ode bi jos dodao vault accoung odnosno schedule atu i inicijalizirao u ovoj instrukciji tu ATAu
    pub system_program: &'a AccountInfo,
    pub token_program: &'a AccountInfo,
}

impl<'a> TryFrom<&'a [AccountInfo]> for InitializeAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        let [initializer, vest_schedule, token_mint, system_program, token_program] = accounts
        else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        SignerAccount::check(&initializer)?;
        ProgramAccount::check_system_program(system_program)?;
        ProgramAccount::check_token_program(token_program)?;
        Mint::check(token_mint)?;

        Ok(Self {
            initializer,
            vest_schedule,
            token_mint,
            system_program,
            token_program,
        })
    }
}

#[repr(C, packed)]
pub struct InitializeInstructionData {
    pub seed: u64,
    pub start_timestamp: u64,
    pub cliff_duration: u64,
    pub total_duration: u64,
    pub step_duration: u64,
    pub bump: u8,
}

impl TryFrom<&[u8]> for InitializeInstructionData {
    type Error = ProgramError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        if data.len() != core::mem::size_of::<InitializeInstructionData>() {
            return Err(ProgramError::InvalidInstructionData);
        }

        let (seed, start_timestamp, cliff_duration, total_duration, step_duration, bump) = (
            u64::from_le_bytes(data[0..8].try_into().unwrap()),
            u64::from_le_bytes(data[8..16].try_into().unwrap()),
            u64::from_le_bytes(data[16..24].try_into().unwrap()),
            u64::from_le_bytes(data[24..32].try_into().unwrap()),
            u64::from_le_bytes(data[32..40].try_into().unwrap()),
            u8::from_le_bytes(data[40..41].try_into().unwrap()),
        );

        if seed == 0 {
            return Err(PinocchioError::InvalidSeed.into());
        }

        let current_timestamp = Clock::get()?.unix_timestamp as u64;
        if start_timestamp < current_timestamp {
            return Err(PinocchioError::StartTimestampInPast.into());
        }

        if cliff_duration >= total_duration
            || step_duration >= total_duration
            || cliff_duration == 0
            || step_duration == 0
        {
            return Err(PinocchioError::InvalidDurations.into());
        }

        if (total_duration - cliff_duration) % step_duration != 0 {
            return Err(PinocchioError::InvalidStepDuration.into());
        }

        Ok(InitializeInstructionData {
            seed,
            start_timestamp,
            cliff_duration,
            total_duration,
            step_duration,
            bump,
        })
    }
}

pub struct Initialize<'a> {
    pub accounts: InitializeAccounts<'a>,
    pub instruction_data: InitializeInstructionData,
}

impl<'a> TryFrom<(&[u8], &'a [AccountInfo])> for Initialize<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&[u8], &'a [AccountInfo])) -> Result<Self, Self::Error> {
        let accounts = InitializeAccounts::try_from(accounts)?;
        let instruction_data = InitializeInstructionData::try_from(data)?;

        let seed_binding = instruction_data.seed.to_le_bytes();

        ProgramAccount::verify(
            &[
                Seed::from(b"vest_schedule"),
                Seed::from(&seed_binding),
                Seed::from(accounts.token_mint.key().as_ref()),
                Seed::from(accounts.initializer.key().as_ref()),
            ],
            accounts.vest_schedule,
            instruction_data.bump,
        )?;

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Initialize<'a> {
    pub const DISCRIMINATOR: &'a u8 = &0;

    pub fn process(&self) -> ProgramResult {
        let seed_binding = self.instruction_data.seed.to_le_bytes();
        let binding = [self.instruction_data.bump];
        let vest_schedule_seed = [
            Seed::from(b"vest_schedule"),
            Seed::from(&seed_binding),
            // nema bas razloga za koristit mint kao dio seeda
            Seed::from(self.accounts.token_mint.key().as_ref()),
            // nema bas razloga za koristit intiializera kao dio seeda
            Seed::from(self.accounts.initializer.key().as_ref()),
            Seed::from(&binding),
        ];

        ProgramAccount::init::<VestSchedule>(
            self.accounts.initializer,
            // dodati provjeru da account nije vec initializiran
            self.accounts.vest_schedule,
            &vest_schedule_seed,
            VestSchedule::LEN,
        )?;

        let mut vest_schedule_data = self.accounts.vest_schedule.try_borrow_mut_data()?;
        let vest_schedule = VestSchedule::load_mut(&mut vest_schedule_data)?;

        vest_schedule.set_inner(
            *self.accounts.token_mint.key(),
            *self.accounts.initializer.key(),
            self.instruction_data.seed,
            self.instruction_data.start_timestamp,
            self.instruction_data.cliff_duration,
            self.instruction_data.total_duration,
            self.instruction_data.step_duration,
            self.instruction_data.bump,
        );

        Ok(())
    }
}
