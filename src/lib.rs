use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::{clock::Clock, Sysvar}, 
};

// Define constants
pub const TOTAL_PLEDGE_SUPPLY: u64 = 100_000_000; // Total supply of Pledge tokens
pub const TOTAL_SOLHIT_SUPPLY: u64 = 14_000_000; // Total supply of Solheist tokens
pub const LOCKED_SOLHIT_TOKENS: u64 = 4_000_000; // Solheist tokens reserved for rewards
pub const VESTING_PERIOD: u64 = 63_072_000; // 2 years in seconds
pub const REWARD_RATE: u64 = 40; // 1 Pledge token = 40 Solheist tokens

// Define sale phases
pub const PHASE_DURATIONS: [u64; 5] = [1296000, 1296000, 1296000, 1296000, u64::MAX];
pub const PHASE_RATES: [u64; 5] = [2, 175, 150, 125, 100];

// Define state variables
pub struct PledgeContract {
    pub total_pledge_supply: u64,
    pub solhit_token_supply: u64,
    pub locked_solhit_tokens: u64,
    pub vesting_period: u64,
    pub reward_rate: u64,
    pub phase_durations: [u64; 5],
    pub phase_rates: [u64; 5],
}

impl PledgeContract {
    pub fn new() -> Self {
        Self {
            total_pledge_supply: TOTAL_PLEDGE_SUPPLY,
            solhit_token_supply: TOTAL_SOLHIT_SUPPLY,
            locked_solhit_tokens: LOCKED_SOLHIT_TOKENS,
            vesting_period: VESTING_PERIOD,
            reward_rate: REWARD_RATE,
            phase_durations: PHASE_DURATIONS,
            phase_rates: PHASE_RATES,
        }
    }
}

// Define user-specific data
#[derive(BorshDeserialize, BorshSerialize)]
pub struct UserState {
    pub locked_pledge_tokens: u64,
    pub solhit_rewards: u64,
    pub lock_start_time: u64,
}

// Entry point
entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let account_info = next_account_info(account_info_iter)?;

    match instruction_data[0] {
        // Handle different instructions
        0 => buy_pledge(
            account_info,
            u64::from_le_bytes(instruction_data[1..9].try_into().unwrap()),
            Clock::get()?.unix_timestamp.try_into().expect("Conversion from i64 to u64 failed"), 
        ),
        1 => update_reward(account_info, Clock::get()?.unix_timestamp.try_into().expect("Conversion from i64 to u64 failed")),
        2 => view_rewards(account_info),
        _ => {
            msg!("Instruction not recognized");
            Err(ProgramError::InvalidInstructionData)
        }
    }
}

// BuyPledge instruction handler
pub fn buy_pledge(
    account_info: &AccountInfo,
    amount: u64,
    current_time: u64,
) -> ProgramResult {
    let mut user_state = UserState::try_from_slice(&account_info.data.borrow())?;
    let pledge_contract = PledgeContract::new();

    // Validate purchase
    let sale_phase = get_sale_phase(current_time, &pledge_contract.phase_durations);
    let rate = pledge_contract.phase_rates[sale_phase];

    // Mint tokens
    let pledge_tokens = (amount * rate) / 100;
    user_state.locked_pledge_tokens += pledge_tokens;
    user_state.lock_start_time = current_time;

    Ok(())
}

// UpdateReward instruction handler
pub fn update_reward(
    account_info: &AccountInfo,
    current_time: u64,
) -> ProgramResult {
    let mut user_state = UserState::try_from_slice(&account_info.data.borrow())?;
    let pledge_contract = PledgeContract::new();

    // Calculate elapsed time
    let elapsed_time = current_time - user_state.lock_start_time;

    // Calculate rewards
    if elapsed_time >= pledge_contract.vesting_period {
        let solhit_rewards = user_state.locked_pledge_tokens * pledge_contract.reward_rate;
        user_state.solhit_rewards += solhit_rewards;
        user_state.lock_start_time = current_time;
    }

    Ok(())
}

// ViewRewards instruction handler
pub fn view_rewards(account_info: &AccountInfo) -> ProgramResult {
    let user_state = UserState::try_from_slice(&account_info.data.borrow())?;

    msg!("Solheist Rewards: {}", user_state.solhit_rewards);

    Ok(())
}

// Helper function to get the current sale phase
fn get_sale_phase(current_time: u64, phase_durations: &[u64; 5]) -> usize {
    let mut elapsed_time = 0;
    for (i, &duration) in phase_durations.iter().enumerate() {
        elapsed_time += duration;
        if current_time < elapsed_time {
            return i;
        }
    }
    phase_durations.len() - 1
}