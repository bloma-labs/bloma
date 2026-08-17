//! Shared token CPI helpers.
//!
//! Every transfer goes through `transfer_checked` with the mint's decimals, so
//! the program works identically with SPL Token and Token-2022 mints. The one
//! burn goes through `burn_checked` for the same reason.
//!
//! Transfers and the burn are different in kind and the file keeps them
//! separate. A transfer moves base asset between accounts this program owns or
//! reaches; the burn destroys $KOLNY supply outright and is reachable from
//! exactly one instruction, `register_forager`.

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, BurnChecked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::errors::ColonyError;

/// Move base asset from a user-owned token account into a program vault.
pub fn transfer_from_user<'info>(
    token_program: &Interface<'info, TokenInterface>,
    from: &InterfaceAccount<'info, TokenAccount>,
    to: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    authority: &Signer<'info>,
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    token_interface::transfer_checked(
        CpiContext::new(
            token_program.to_account_info(),
            TransferChecked {
                from: from.to_account_info(),
                mint: mint.to_account_info(),
                to: to.to_account_info(),
                authority: authority.to_account_info(),
            },
        ),
        amount,
        mint.decimals,
    )
}

/// Destroy tokens out of a user-owned account, permanently lowering the mint's
/// supply.
///
/// `burn_checked` rather than `burn`: it carries the mint's decimals and the
/// token program rejects the instruction when they disagree, so a client that
/// scaled an amount against the wrong decimals fails instead of destroying a
/// thousand times too much or too little. That failure has already happened in
/// this project once, in the CLI, where a hard-coded 6 against a 9-decimal
/// asset sent a thousandth of the intended amount.
///
/// Unlike the transfer helpers, a zero amount is an ERROR here and not a silent
/// no-op. A zero-amount admission burn would let registration keep succeeding
/// while nothing was destroyed, and nothing on-chain would distinguish that
/// from a working colony. `MIN_ADMISSION_BURN_BASE_UNITS` already puts zero outside the
/// configurable range; this makes it unreachable even if that bound is ever
/// loosened, which is the difference between writing the rule down and
/// enforcing it.
pub fn burn_from_user<'info>(
    token_program: &Interface<'info, TokenInterface>,
    mint: &InterfaceAccount<'info, Mint>,
    from: &InterfaceAccount<'info, TokenAccount>,
    authority: &Signer<'info>,
    amount: u64,
) -> Result<()> {
    require!(amount > 0, ColonyError::ZeroAmount);
    token_interface::burn_checked(
        CpiContext::new(
            token_program.to_account_info(),
            BurnChecked {
                mint: mint.to_account_info(),
                from: from.to_account_info(),
                authority: authority.to_account_info(),
            },
        ),
        amount,
        mint.decimals,
    )
}

/// Move base asset out of a program vault, signed by the vault's owning PDA.
///
/// `signer_seeds` must list the seeds of the PDA that owns `from`, in the same
/// order used to derive it, with the stored bump last.
pub fn transfer_signed<'info>(
    token_program: &Interface<'info, TokenInterface>,
    from: &InterfaceAccount<'info, TokenAccount>,
    to: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    authority: &AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    token_interface::transfer_checked(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            TransferChecked {
                from: from.to_account_info(),
                mint: mint.to_account_info(),
                to: to.to_account_info(),
                authority: authority.clone(),
            },
            signer_seeds,
        ),
        amount,
        mint.decimals,
    )
}
