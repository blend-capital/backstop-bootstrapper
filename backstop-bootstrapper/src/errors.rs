use soroban_sdk::contracterror;

/// The error codes for the contract.
#[contracterror]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum BackstopBootstrapperError {
    // Default errors to align with built-in contract
    InternalError = 1,
    AlreadyInitializedError = 3,

    UnauthorizedError = 4,

    NegativeAmountError = 8,
    AllowanceError = 9,
    BalanceError = 10,
    OverflowError = 12,

    BadRequest = 50,

    DurationTooShort = 100,
    DurationTooLong = 101,
    InvalidBootstrapAmount = 102,
    InvalidBootstrapWeight = 103,
    BootstrapNotFoundError = 104,
    BootstrapNotActiveError = 105,
    BootstrapNotCompleteError = 106,
    BootstrapAlreadyClaimedError = 107,
    InsufficientDepositError = 108,
}
