use pinocchio::{program_error::ProgramError, pubkey::Pubkey};

#[repr(C, packed)]
pub struct VestSchedule {
    token_mint: Pubkey,
    authority: Pubkey,
    seed: u64,
    start_timestamp: u64,
    cliff_duration: u64,
    total_duration: u64,
    step_duration: u64,
    bump: u8,
}

#[repr(u8)]
pub enum VestStatus {
    NotStarted = 0u8,
    Cliff = 1u8,
    Stepping = 2u8,
    Completed = 3u8,
}

impl VestSchedule {
    pub const LEN: usize = size_of::<Pubkey>() * 2 + size_of::<u64>() * 5 + size_of::<u8>();

    #[inline(always)]
    pub fn load_mut(bytes: &mut [u8]) -> Result<&mut Self, ProgramError> {
        if bytes.len() != VestSchedule::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &mut *core::mem::transmute::<*mut u8, *mut Self>(bytes.as_mut_ptr()) })
    }

    #[inline(always)]
    pub fn load(bytes: &[u8]) -> Result<&Self, ProgramError> {
        if bytes.len() != VestSchedule::LEN {
            return Err(ProgramError::InvalidAccountData);
        }

        Ok(unsafe { &*core::mem::transmute::<*const u8, *const Self>(bytes.as_ptr()) })
    }

    #[inline(always)]
    pub fn calculate_claimable_amount(
        &self,
        current_timestamp: u64,
        total_allocated_amount: u64,
        claimed_amount: u64,
    ) -> u64 {
        if current_timestamp < self.start_timestamp + self.cliff_duration {
            return 0;
        }

        if current_timestamp >= self.start_timestamp + self.total_duration {
            return total_allocated_amount.saturating_sub(claimed_amount);
        }

        let elapsed_time = current_timestamp.saturating_sub(self.start_timestamp);
        let steps_elapsed = elapsed_time.saturating_sub(self.cliff_duration) / self.step_duration;
        let total_steps = (self.total_duration - self.cliff_duration) / self.step_duration;

        let vested_amount = (total_allocated_amount as u128)
            .saturating_mul(steps_elapsed as u128)
            .saturating_div(total_steps as u128) as u64;

        vested_amount.saturating_sub(claimed_amount)
    }
    
    #[inline(always)]
    pub fn is_cliff_completed(&self, current_timestamp: u64) -> bool {
        current_timestamp >= self.start_timestamp + self.cliff_duration
    }

    #[inline(always)]
    pub fn token_mint(&self) -> &Pubkey {
        &self.token_mint
    }

    #[inline(always)]
    pub fn authority(&self) -> &Pubkey {
        &self.authority
    }

    #[inline(always)]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    #[inline(always)]
    pub fn start_timestamp(&self) -> u64 {
        self.start_timestamp
    }

    #[inline(always)]
    pub fn cliff_duration(&self) -> u64 {
        self.cliff_duration
    }

    #[inline(always)]
    pub fn total_duration(&self) -> u64 {
        self.total_duration
    }

    #[inline(always)]
    pub fn step_duration(&self) -> u64 {
        self.step_duration
    }

    #[inline(always)]
    pub fn bump(&self) -> u8 {
        self.bump
    }

    #[inline(always)]
    pub fn set_inner(
        &mut self,
        token_mint: Pubkey,
        authority: Pubkey,
        seed: u64,
        start_timestamp: u64,
        cliff_duration: u64,
        total_duration: u64,
        step_duration: u64,
        bump: u8,
    ) {
        self.token_mint = token_mint;
        self.authority = authority;
        self.seed = seed;
        self.start_timestamp = start_timestamp;
        self.cliff_duration = cliff_duration;
        self.total_duration = total_duration;
        self.step_duration = step_duration;
        self.bump = bump;
    }
}
