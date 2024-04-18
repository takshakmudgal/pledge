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
use std::convert::TryInto;

// Define constants
pub const TOTAL_PLEDGE_SUPPLY: u64 = 100_000_000;
pub const TOTAL_SOLHIT_SUPPLY: u64 = 14_000_000;
pub const LOCKED_SOLHIT_TOKENS: u64 = 4_000_000;
pub const VESTING_PERIOD: u64 = 63_072_000;
pub const REWARD_RATE: u64 = 40;

pub const PHASE_DURATIONS: [u64; 5] = [1_296_000, 1_296_000, 1_296_000, 1_296_000, u64::MAX];
pub const PHASE_RATES: [u64; 5] = [200, 175, 150, 125, 100];

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

pub struct UserState {
    pub locked_pledge_tokens: u64,
    pub solhit_rewards: u64,
    pub lock_start_time: u64,
    pub vesting_end_time: u64,
}

impl BorshSerialize for UserState {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.locked_pledge_tokens.serialize(writer)?;
        self.solhit_rewards.serialize(writer)?;
        self.lock_start_time.serialize(writer)?;
        self.vesting_end_time.serialize(writer)
    }
}

impl BorshDeserialize for UserState {
    fn deserialize(buf: &mut &[u8]) -> std::io::Result<Self> {
        let locked_pledge_tokens = u64::deserialize(buf)?;
        let solhit_rewards = u64::deserialize(buf)?;
        let lock_start_time = u64::deserialize(buf)?;
        let vesting_end_time = u64::deserialize(buf)?;

        Ok(Self {
            locked_pledge_tokens,
            solhit_rewards,
            lock_start_time,
            vesting_end_time,
        })
    }

    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut buf = vec![];
        reader.read_to_end(&mut buf)?;
        Self::deserialize(&mut buf.as_slice())
    }
}

entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let account_info = next_account_info(account_info_iter)?;

    match instruction_data[0] {
        0 => buy_pledge(
            account_info,
            u64::from_le_bytes(instruction_data[1..9].try_into().unwrap()),
            Clock::get()?.unix_timestamp.try_into().expect("Conversion from i64 to u64 failed"), 
        ),
        1 => update_reward(account_info, Clock::get()?.unix_timestamp.try_into().expect("Conversion from i64 to u64 failed")),
        2 => view_rewards(account_info),
        3 => claim_rewards(
            &accounts,
        ),
        _ => {
            msg!("Instruction not recognized");
            Err(ProgramError::InvalidInstructionData)
        }
    }
}


pub fn buy_pledge(
    account_info: &AccountInfo,
    amount: u64,
    current_time: u64,
) -> ProgramResult {
    let mut user_state = UserState::try_from_slice(&account_info.data.borrow())?;
    let pledge_contract = PledgeContract::new();

    let sale_phase = get_sale_phase(current_time, &pledge_contract.phase_durations);
    let rate = pledge_contract.phase_rates[sale_phase];

    let pledge_tokens = (amount * rate) / 100;

    if pledge_tokens > pledge_contract.total_pledge_supply - user_state.locked_pledge_tokens {
        return Err(ProgramError::InvalidArgument);
    }

    user_state.locked_pledge_tokens += pledge_tokens;
    user_state.lock_start_time = current_time;
    user_state.vesting_end_time = user_state.vesting_end_time.max(current_time + pledge_contract.vesting_period);

    let serialized_user_state = serialize_user_state(&user_state)?;
    account_info.data.borrow_mut().copy_from_slice(&serialized_user_state);

    Ok(())
}

pub fn update_reward(
    account_info: &AccountInfo,
    current_time: u64,
) -> ProgramResult {
    let mut user_state = UserState::try_from_slice(&account_info.data.borrow())?;
    let pledge_contract = PledgeContract::new();

    let elapsed_time = current_time.saturating_sub(user_state.lock_start_time);

    if elapsed_time >= pledge_contract.vesting_period {
        let solhit_rewards = (user_state.locked_pledge_tokens as u128 * pledge_contract.reward_rate as u128) as u64;
        user_state.solhit_rewards = user_state.solhit_rewards.saturating_add(solhit_rewards);
        user_state.lock_start_time = current_time;
        unlock_vested_tokens(&mut user_state);
    } else if current_time >= user_state.vesting_end_time {
        unlock_vested_tokens(&mut user_state);
    }

    let serialized_user_state = serialize_user_state(&user_state)?;
    account_info.data.borrow_mut().copy_from_slice(&serialized_user_state);

    Ok(())
}

fn unlock_vested_tokens(user_state: &mut UserState) {
    user_state.locked_pledge_tokens = 0;
    user_state.vesting_end_time = 0;
}

pub fn view_rewards(account_info: &AccountInfo) -> ProgramResult {
    let user_state = UserState::try_from_slice(&account_info.data.borrow())?;

    msg!("Solheist Rewards: {}", user_state.solhit_rewards);

    Ok(())
}

pub fn claim_rewards(
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let account_info = next_account_info(account_info_iter)?;

    let user_state = UserState::try_from_slice(&account_info.data.borrow())?;
    let pledge_contract = PledgeContract::new();

    if user_state.solhit_rewards == 0 {
        msg!("No rewards to claim");
        return Ok(());
    }

    let solhit_token_account_info = next_account_info(account_info_iter)?;

    let transfer_to_user_amount = user_state.solhit_rewards;
    let remaining_solhit_tokens = pledge_contract.solhit_token_supply.saturating_sub(pledge_contract.locked_solhit_tokens);

    if transfer_to_user_amount > remaining_solhit_tokens {
        msg!("Not enough Solheist tokens in the contract");
        return Err(ProgramError::InsufficientFunds);
    }

    // Transfer Solheist tokens to the user
    solana_program::program::invoke_signed(
        &solana_program::system_instruction::transfer(
            &solhit_token_account_info.key,
            account_info.key,
            transfer_to_user_amount,
        ),
        &[solhit_token_account_info.clone(), account_info.clone()],
        &[],
    )?;

    let mut user_state = UserState::try_from_slice(&account_info.data.borrow())?;
    user_state.solhit_rewards = 0;

    let serialized_user_state = serialize_user_state(&user_state)?;
    account_info.data.borrow_mut().copy_from_slice(&serialized_user_state);

    msg!("Rewards claimed successfully");

    Ok(())
}


fn serialize_user_state(user_state: &UserState) -> Result<Vec<u8>, ProgramError> {
    let mut buf = vec![];
    user_state.serialize(&mut buf)?;
    Ok(buf)
}

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
