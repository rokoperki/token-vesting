use pinocchio::pubkey::Pubkey;

#[repr(C, packed)]
pub struct VestParticipant {
    pub discriminator: u8,
    pub participant: Pubkey,
    pub schedule: Pubkey,
    pub allocated_amount: u64,
    pub claimed_amount: u64,
    pub bump: u8,
}

use crate::Discriminator;

impl Discriminator for VestParticipant {
    const LEN: usize = Self::LEN;
    const DISCRIMINATOR: u8 = Self::DISCRIMINATOR;
}

impl VestParticipant {
    pub const LEN: usize = std::mem::size_of::<Pubkey>() * 2
        + std::mem::size_of::<u64>() * 2
        + std::mem::size_of::<u8>() * 2;
    pub const DISCRIMINATOR: u8 = 1;

    #[inline(always)]
    pub fn load_mut(bytes: &mut [u8]) -> Result<&mut Self, pinocchio::program_error::ProgramError> {
        if bytes.len() != VestParticipant::LEN {
            return Err(pinocchio::program_error::ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &mut *core::mem::transmute::<*mut u8, *mut Self>(bytes.as_mut_ptr()) })
    }

    #[inline(always)]
    pub fn load(bytes: &[u8]) -> Result<&Self, pinocchio::program_error::ProgramError> {
        if bytes.len() != VestParticipant::LEN {
            return Err(pinocchio::program_error::ProgramError::InvalidAccountData);
        }

        Ok(unsafe { &*core::mem::transmute::<*const u8, *const Self>(bytes.as_ptr()) })
    }

    #[inline(always)]
    pub fn participant(&self) -> &Pubkey {
        &self.participant
    }

    #[inline(always)]
    pub fn schedule(&self) -> &Pubkey {
        &self.schedule
    }

    #[inline(always)]
    pub fn allocated_amount(&self) -> u64 {
        self.allocated_amount
    }

    #[inline(always)]
    pub fn claimed_amount(&self) -> u64 {
        self.claimed_amount
    }

    pub fn bump(&self) -> u8 {
        self.bump
    }

    pub fn set_claimed_amount(&mut self, amount: u64) {
        self.claimed_amount = amount;
    }

    pub fn set_inner(
        &mut self,
        participant: Pubkey,
        schedule: Pubkey,
        allocated_amount: u64,
        claimed_amount: u64,
        bump: u8,
    ) {
        self.discriminator = VestParticipant::DISCRIMINATOR;
        self.participant = participant;
        self.schedule = schedule;
        self.allocated_amount = allocated_amount;
        self.claimed_amount = claimed_amount;
        self.bump = bump;
    }
}
