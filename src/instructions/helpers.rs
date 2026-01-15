use pinocchio::{
    account_info::AccountInfo,
    instruction::{Seed, Signer},
    program_error::ProgramError,
    pubkey::{find_program_address, Pubkey},
    sysvars::{rent::Rent, Sysvar},
    ProgramResult,
};
use pinocchio_associated_token_account::instructions::Create;
use pinocchio_system::instructions::CreateAccount;

use crate::PinocchioError;

pub struct SignerAccount;

impl SignerAccount {
    pub fn check(
        account: &pinocchio::account_info::AccountInfo,
    ) -> Result<(), pinocchio::program_error::ProgramError> {
        if !account.is_signer() {
            return Err(pinocchio::program_error::ProgramError::MissingRequiredSignature);
        }
        Ok(())
    }
}

pub struct ProgramAccount;

impl ProgramAccount {
    pub fn check_system_program(
        account: &pinocchio::account_info::AccountInfo,
    ) -> Result<(), pinocchio::program_error::ProgramError> {
        if account.key() != &pinocchio_system::ID {
            return Err(PinocchioError::InvalidOwner.into());
        }
        Ok(())
    }

    pub fn check_token_program(
        account: &pinocchio::account_info::AccountInfo,
    ) -> Result<(), pinocchio::program_error::ProgramError> {
        if account.key() != &pinocchio_token::ID {
            return Err(PinocchioError::InvalidOwner.into());
        }
        Ok(())
    }

    pub fn check_ata_program(
        account: &pinocchio::account_info::AccountInfo,
    ) -> Result<(), pinocchio::program_error::ProgramError> {
        if account.key() != &pinocchio_associated_token_account::ID {
            return Err(PinocchioError::InvalidOwner.into());
        }
        Ok(())
    }
}

pub trait Discriminator {
    const LEN: usize;
    const DISCRIMINATOR: u8;
}

impl ProgramAccount {
    pub fn check<T: Discriminator>(account: &AccountInfo) -> Result<(), ProgramError> {
        if !account.is_owned_by(&crate::ID) {
            return Err(PinocchioError::InvalidOwner.into());
        }

        if account.data_len() != T::LEN {
            return Err(PinocchioError::InvalidAccountData.into());
        }

        let data = account.try_borrow_data()?;
        if data[0] != T::DISCRIMINATOR {
            return Err(PinocchioError::InvalidDiscriminator.into());
        }

        Ok(())
    }

    pub fn verify(seeds: &[Seed], account: &AccountInfo, bump: u8) -> Result<(), ProgramError> {
        let seed_bytes: Vec<&[u8]> = seeds.iter().map(|s| s.as_ref()).collect();

        let (expected_pubkey, expected_bump) = find_program_address(&seed_bytes, &crate::ID);

        if *account.key() != expected_pubkey {
            return Err(ProgramError::InvalidAccountData);
        }

        if bump != expected_bump {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(())
    }

    pub fn init<'a, T: Sized>(
        payer: &AccountInfo,
        account: &AccountInfo,
        seeds: &[Seed<'a>],
        space: usize,
    ) -> ProgramResult {
        if account.lamports() > 0 {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        let lamports = Rent::get()?.minimum_balance(space);
        let signer = [Signer::from(seeds)];

        CreateAccount {
            from: payer,
            to: account,
            lamports,
            space: space as u64,
            owner: &crate::ID,
        }
        .invoke_signed(&signer)?;

        Ok(())
    }
}

pub struct Mint;

impl Mint {
    pub fn check(account: &AccountInfo) -> Result<(), ProgramError> {
        if !account.is_owned_by(&pinocchio_token::ID) {
            return Err(PinocchioError::InvalidOwner.into());
        }

        if account.data_len().ne(&pinocchio_token::state::Mint::LEN) {
            return Err(PinocchioError::InvalidAccountData.into());
        }

        Ok(())
    }
}

pub struct Token;
impl Token {
    pub fn check(account: &AccountInfo) -> Result<(), ProgramError> {
        if !account.is_owned_by(&pinocchio_token::ID) {
            return Err(PinocchioError::InvalidOwner.into());
        }

        if account
            .data_len()
            .ne(&pinocchio_token::state::TokenAccount::LEN)
        {
            return Err(PinocchioError::InvalidAccountData.into());
        }

        Ok(())
    }
}

pub struct AssociatedToken;

impl AssociatedToken {
    pub fn check(
        account: &AccountInfo,
        authority: Pubkey,
        mint: Pubkey,
        token_program: Pubkey,
    ) -> Result<(), ProgramError> {
        Token::check(account)?;

        let (expected_ata, _bump) = find_program_address(
            &[authority.as_ref(), token_program.as_ref(), mint.as_ref()],
            &pinocchio_associated_token_account::ID,
        );

        if *account.key() != expected_ata {
            return Err(PinocchioError::InvalidAccountData.into());
        }

        if account.lamports() == 0 {
            return Err(PinocchioError::UninitializedAccount.into());
        }

        Ok(())
    }

    pub fn init(
        account: &AccountInfo,
        mint: &AccountInfo,
        payer: &AccountInfo,
        owner: &AccountInfo,
        system_program: &AccountInfo,
        token_program: &AccountInfo,
    ) -> ProgramResult {
        Create {
            funding_account: payer,
            account,
            wallet: owner,
            mint,
            system_program,
            token_program,
        }
        .invoke()
    }

    pub fn init_if_needed(
        account: &AccountInfo,
        mint: &AccountInfo,
        payer: &AccountInfo,
        owner: &AccountInfo,
        system_program: &AccountInfo,
        token_program: &AccountInfo,
    ) -> ProgramResult {
        match Self::check(account, *payer.key(), *mint.key(), *token_program.key()) {
            Ok(_) => Ok(()),
            Err(_) => Self::init(account, mint, payer, owner, system_program, token_program),
        }
    }
}
