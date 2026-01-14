use {pinocchio::program_error::ProgramError, thiserror::Error};

#[derive(Debug, Error)]
pub enum PinocchioError {
    #[error("Lamport balance below rent-exempt threshold")]
    NotRentExempt,
    #[error("Invalid Owner")]
    InvalidOwner,
    #[error("Invalid Account Data")]
    InvalidAccountData,
    #[error("Invalid Adress ")]
    InvalidAddress,
    #[error("Uninitialized Account")]
    UninitializedAccount,
    #[error("No claimable amount available")]
    NoClaimableAmount,
    #[error("Vesting schedule has not started yet")]
    StartTimestampInPast,
    #[error("Invalid ")]
    InvalidDurations,
    #[error("Step duration must divide total duration evenly")]
    InvalidStepDuration,
}

impl From<PinocchioError> for ProgramError {
    fn from(error: PinocchioError) -> Self {
        ProgramError::Custom((error as u64).try_into().unwrap())
    }
}
