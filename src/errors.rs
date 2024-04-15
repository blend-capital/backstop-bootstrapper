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

    InvalidCloseLedger = 100,
    InvalidBootstrapToken = 101,
    InvalidBootstrapAmount = 102,
    InvalidPoolAddressError = 103,
    InvalidBootstrapStatus = 104,
    AlreadyClaimedError = 105,
    InsufficientDepositError = 106,
    ReceivedNoBackstopTokens = 107,
    AlreadyRefundedError = 108,
}
