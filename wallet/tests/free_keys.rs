mod utils;

use self::utils::State;
use chain_crypto::{bech32::Bech32, SecretKey};
use chain_impl_mockchain::{
    certificate::VoteCast,
    fragment::Fragment,
    value::Value,
    vote::{Choice, Payload},
};
use std::convert::TryInto;
use wallet::transaction::dump_free_utxo;

const BLOCK0: &[u8] = include_bytes!("../../test-vectors/block0");
const ACCOUNT: &str = include_str!("../../test-vectors/free_keys/key1.prv");
const UTXO1: &str = include_str!("../../test-vectors/free_keys/key2.prv");
const UTXO2: &str = include_str!("../../test-vectors/free_keys/key3.prv");
const WALLET_VALUE: Value = Value(10_000 + 1000);

///
#[test]
fn free_keys1() {
    let builder = wallet::RecoveryBuilder::new();

    let builder = builder
        .account_secret_key(SecretKey::try_from_bech32_str(String::from(ACCOUNT).trim()).unwrap());

    let builder = [UTXO1, UTXO2].iter().fold(builder, |builder, key| {
        builder.add_key(SecretKey::try_from_bech32_str(String::from(*key).trim()).unwrap())
    });

    let mut free_keys = builder.build_free_utxos().unwrap();

    let account = builder.build_wallet().unwrap();

    let mut state = State::new(BLOCK0);
    let settings = state.settings().expect("valid initial settings");
    let address = account.account_id().address(settings.discrimination());

    for fragment in state.initial_contents() {
        free_keys.check_fragment(&fragment.hash(), fragment);
    }

    assert_eq!(free_keys.unconfirmed_value(), Some(WALLET_VALUE));

    let (fragment, ignored) = dump_free_utxo(&settings, &address, &mut free_keys)
        .next()
        .unwrap();

    assert!(ignored.is_empty());

    state
        .apply_fragments(&[fragment.to_raw()])
        .expect("the dump fragments should be valid");
}

#[test]
fn cast_vote() {
    impl State {
        pub fn get_account_state(
            &self,
            account_id: wallet::AccountId,
        ) -> Option<(chain_impl_mockchain::account::SpendingCounter, Value)> {
            self.ledger
                .accounts()
                .get_state(&chain_crypto::PublicKey::from(account_id).into())
                .ok()
                .map(|account_state| (account_state.counter, account_state.value))
        }
    }

    let builder = wallet::RecoveryBuilder::new();

    let builder = builder
        .account_secret_key(SecretKey::try_from_bech32_str(String::from(ACCOUNT).trim()).unwrap());

    let builder = [UTXO1, UTXO2].iter().fold(builder, |builder, key| {
        builder.add_key(SecretKey::try_from_bech32_str(String::from(*key).trim()).unwrap())
    });

    let mut free_keys = builder.build_free_utxos().unwrap();

    let mut account = builder.build_wallet().expect("recover account");

    let mut state = State::new(BLOCK0);
    let settings = state.settings().expect("valid initial settings");
    let address = account.account_id().address(settings.discrimination());

    for fragment in state.initial_contents() {
        free_keys.check_fragment(&fragment.hash(), fragment);
    }

    assert_eq!(free_keys.unconfirmed_value(), Some(WALLET_VALUE));

    let (fragment, ignored) = dump_free_utxo(&settings, &address, &mut free_keys)
        .next()
        .unwrap();

    assert!(ignored.is_empty());

    account.check_fragment(&fragment.hash(), &fragment);

    state
        .apply_fragments(&[fragment.to_raw()])
        .expect("the dump fragments should be valid");

    let (_counter, value) = state.get_account_state(account.account_id()).unwrap();

    account.confirm(&fragment.hash());

    assert_eq!(account.confirmed_value(), value);

    let vote_plan_id: [u8; 32] = hex::decode(
        "4aa30d9df6d2dfdb45725c0de00d1a73394950c8bf3dabc8285f46f1e25e53fa",
    )
    .unwrap()[..]
        .try_into()
        .unwrap();

    let index = 0;
    let choice = Choice::new(1);

    let payload = Payload::Public { choice };

    let cast = VoteCast::new(vote_plan_id.into(), index, payload);

    let mut builder = wallet::TransactionBuilder::new(&settings, cast);

    let value = builder.estimate_fee_with(1, 0);

    let account_tx_builder = account.new_transaction(value).unwrap();
    let input = account_tx_builder.input();
    let witness_builder = account_tx_builder.witness_builder();

    builder.add_input(input, witness_builder);

    let tx = builder.finalize_tx(()).unwrap();

    let fragment = Fragment::VoteCast(tx);
    let raw = fragment.to_raw();
    let id = raw.id();

    account_tx_builder.add_fragment_id(id);

    state
        .apply_fragments(&[raw])
        .expect("couldn't apply votecast fragment");

    account.confirm(&id);

    let (_counter, value) = state.get_account_state(account.account_id()).unwrap();
    assert_eq!(account.confirmed_value(), value);
}
