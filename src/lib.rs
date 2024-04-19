use borsh::{BorshDeserialize, BorshSerialize};
use borsh::io::Write;
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
    fn serialize<W: Write>(&self, writer: &mut W) -> std::result::Result<(), std::io::Error> {
        self.locked_pledge_tokens.serialize(writer)?;
        self.solhit_rewards.serialize(writer)?;
        self.lock_start_time.serialize(writer)?;
        self.vesting_end_time.serialize(writer)?;
        Ok(())
    }
}

impl BorshDeserialize for UserState {
    fn deserialize(buf: &mut &[u8]) -> std::result::Result<Self, std::io::Error> {
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

    emit_event(PledgeEvent::Purchase(amount, rate, user_state.locked_pledge_tokens));

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
        println!("Calculated solhit_rewards: {}", solhit_rewards);  // Debug print
        user_state.solhit_rewards = user_state.solhit_rewards.saturating_add(solhit_rewards);
        println!("Updated solhit_rewards in UserState: {}", user_state.solhit_rewards);  // Debug print
        user_state.lock_start_time = current_time;
        unlock_vested_tokens(&mut user_state);
    } else if current_time >= user_state.vesting_end_time {
        unlock_vested_tokens(&mut user_state);
    }

    let serialized_user_state = serialize_user_state(&user_state)?;
    account_info.data.borrow_mut().copy_from_slice(&serialized_user_state);

    emit_event(PledgeEvent::RewardUpdate(user_state.solhit_rewards, elapsed_time));

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
    emit_event(PledgeEvent::RewardClaim(user_state.solhit_rewards));

    Ok(())
}


fn serialize_user_state(user_state: &UserState) -> Result<Vec<u8>, ProgramError> {
    let mut buf = vec![];
    user_state.serialize(&mut buf)?;
    println!("Serialized UserState: {:?}", buf);  // Debug print
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

pub enum PledgeEvent {
    Purchase(u64, u64, u64), // amount, rate, total_pledge_tokens
    RewardUpdate(u64, u64), // solhit_rewards, elapsed_time
    RewardClaim(u64),       // solhit_rewards
}

pub fn emit_event(event: PledgeEvent) {
    let event_data = match event {
        PledgeEvent::Purchase(amount, rate, total_pledge_tokens) => {
            format!("Pledge tokens purchased: {} at rate {} for total: {}", amount, rate, total_pledge_tokens)
        },
        PledgeEvent::RewardUpdate(solhit_rewards, elapsed_time) => {
            format!("Rewards updated: Solheist Rewards: {} after elapsed time: {}", solhit_rewards, elapsed_time)
        },
        PledgeEvent::RewardClaim(solhit_rewards) => {
            format!("Rewards claimed: Solheist Rewards: {}", solhit_rewards)
        },
    };

    msg!("{}", event_data);
    solana_program::log::sol_log(&event_data);
}


#[cfg(test)]
mod tests {
    use super::*;    
use crate::{buy_pledge, UserState, PledgeContract};
use solana_program::{pubkey::Pubkey, account_info::AccountInfo};


    #[test]
fn test_buy_pledge() {
    let mut account_data = vec![0u8; std::mem::size_of::<UserState>()];
    let pubkey1 = Pubkey::new_unique();
    let pubkey2 = Pubkey::new_unique();
    let mut lamports = 0;
    let account_info = AccountInfo::new(
        &pubkey1,
        false,
        true,
        &mut lamports,
        &mut account_data,
        &pubkey2,
        false,
        0,
    );

    let amount = 1000;
    let current_time = 1_000_000;
    let result = buy_pledge(&account_info, amount, current_time);
    assert!(result.is_ok());

    let user_state = UserState::try_from_slice(&account_info.data.borrow()).unwrap();
    let pledge_contract = PledgeContract::new();
    let sale_phase = get_sale_phase(current_time, &pledge_contract.phase_durations);
    let rate = pledge_contract.phase_rates[sale_phase];
    let expected_pledge_tokens = (amount * rate) / 100;

    assert_eq!(user_state.locked_pledge_tokens, expected_pledge_tokens);
    assert_eq!(user_state.lock_start_time, current_time);
    assert_eq!(user_state.vesting_end_time, current_time + pledge_contract.vesting_period);
}
#[test]
fn test_buy_pledge_vesting_period() {
  let mut account_data = vec![0u8; std::mem::size_of::<UserState>()];
  let pubkey = Pubkey::new_unique();
  let mut lamports = 1000;
  let account_info = AccountInfo::new(
    &pubkey,
    false,
    true,
    &mut lamports,
    &mut account_data,
    &pubkey,
    false,
    0,
  );

  let amount = 500;
  let current_time = 1_000_000;

  let _result = buy_pledge(&account_info, amount, current_time);

  let user_state = UserState::try_from_slice(&account_info.data.borrow()).unwrap();
  let pledge_contract = PledgeContract::new();

  assert_eq!(user_state.vesting_end_time, current_time + pledge_contract.vesting_period);
}

#[test]
fn test_buy_pledge_exceed_supply() {
  let mut account_data = vec![0u8; std::mem::size_of::<UserState>()];
  let pubkey = Pubkey::new_unique();
  let mut lamports = 1000;
  let account_info = AccountInfo::new(
    &pubkey,
    false,
    true,
    &mut lamports,
    &mut account_data,
    &pubkey,
    false,
    0,
  );

  let pledge_contract = PledgeContract::new();
  let amount = pledge_contract.total_pledge_supply + 1;
  let current_time = 1_000_000;

  let result = buy_pledge(&account_info, amount, current_time);

  assert!(result.is_err());
}

#[test]
fn test_buy_pledge_invalid_amount() {
  let mut account_data = vec![0u8; std::mem::size_of::<UserState>()];
  let pubkey = Pubkey::new_unique();
  let mut lamports = 1000;
  let account_info = AccountInfo::new(
    &pubkey,
    false,
    true,
    &mut lamports,
    &mut account_data,
    &pubkey,
    false,
    0,
  );

  let amount = 0;
  let current_time = 1_000_000;

  let result = buy_pledge(&account_info, amount, current_time);

  assert!(result.is_ok());
}

}