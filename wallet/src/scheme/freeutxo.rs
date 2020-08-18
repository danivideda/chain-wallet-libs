use crate::{
    scheme::{on_tx_input, on_tx_output},
    states::{States, Status},
    store::UtxoStore,
};
use chain_crypto::{Ed25519, Ed25519Extended, PublicKey, SecretKey};
use chain_impl_mockchain::{
    fragment::{Fragment, FragmentId},
    transaction::{InputEnum, UtxoPointer},
    value::Value,
};
use chain_path_derivation::{AnyScheme, DerivationPath};
use ed25519_bip32::DerivationScheme;
use hdkeygen::Key;

pub struct Wallet {
    state: States<FragmentId, UtxoStore<SecretKey<Ed25519Extended>, AnyScheme>>,
    keys: Vec<SecretKey<Ed25519Extended>>,
}

impl Wallet {
    pub fn from_keys(keys: Vec<SecretKey<Ed25519Extended>>) -> Self {
        Wallet {
            keys,
            state: States::new(FragmentId::zero_hash(), UtxoStore::new()),
        }
    }

    /// confirm a pending transaction
    ///
    /// to only do once it is confirmed a transaction is on chain
    /// and is far enough in the blockchain history to be confirmed
    /// as immutable
    ///
    pub fn confirm(&mut self, fragment_id: &FragmentId) {
        self.state.confirm(fragment_id)
    }

    /// get the confirmed value of the wallet
    pub fn confirmed_value(&self) -> Value {
        self.state.confirmed_state().1.total_value()
    }

    /// get the unconfirmed value of the wallet
    ///
    /// if `None`, it means there is no unconfirmed state of the wallet
    /// and the value can be known from `confirmed_value`.
    ///
    /// The returned value is the value we expect to see at some point on
    /// chain once all transactions are on chain confirmed.
    pub fn unconfirmed_value(&self) -> Option<Value> {
        let (k, s, _) = self.state.last_state();
        let (kk, _) = self.state.confirmed_state();

        if k == kk {
            None
        } else {
            Some(s.total_value())
        }
    }

    /// get all the pending transactions of the wallet
    ///
    /// If empty it means there's no pending transactions waiting confirmation
    ///
    pub fn pending_transactions(&self) -> impl Iterator<Item = &FragmentId> {
        self.state.iter().filter_map(|(k, _, status)| {
            if status == Status::Pending {
                Some(k)
            } else {
                None
            }
        })
    }

    /// get the utxos of this given wallet
    pub fn utxos(&self) -> &UtxoStore<SecretKey<Ed25519Extended>, AnyScheme> {
        self.state.last_state().1
    }

    fn check(&self, pk: &PublicKey<Ed25519>) -> Option<(usize, SecretKey<Ed25519Extended>)> {
        self.keys
            .iter()
            .cloned()
            .enumerate()
            .find(|(_i, k)| &k.to_public() == pk)
    }

    pub fn check_fragment(&mut self, fragment_id: &FragmentId, fragment: &Fragment) -> bool {
        if self.state.contains(fragment_id) {
            return true;
        }

        let mut at_least_one_match = false;

        let (_, store, _) = self.state.last_state();

        let mut store = store.clone();

        match fragment {
            Fragment::Initial(_config_params) => {}
            Fragment::UpdateProposal(_update_proposal) => {}
            Fragment::UpdateVote(_signed_update) => {}
            Fragment::OldUtxoDeclaration(_utxos) => {}
            _ => {
                on_tx_input(fragment, |input| {
                    if let InputEnum::UtxoInput(pointer) = input.to_enum() {
                        if let Some(spent) = store.remove(&pointer) {
                            at_least_one_match = true;
                            store = spent;
                        }
                    }
                });

                on_tx_output(fragment, |(index, output)| {
                    use chain_addr::Kind::{Group, Single};
                    let pk = match output.address.kind() {
                        Single(pk) => Some(pk),
                        Group(pk, _) => {
                            // TODO: the account used for the group case
                            // needs to be checked and handled
                            Some(pk)
                        }
                        _ => None,
                    };
                    if let Some(pk) = pk {
                        if let Some((fake_derivation, key)) = self.check(pk) {
                            let pointer = UtxoPointer {
                                transaction_id: *fragment_id,
                                output_index: index as u8,
                                value: output.value,
                            };

                            // HACK: this is wrong, but for now I'll leave it
                            // what happens is that if I don't add any derivation
                            // all the utxos get in the same group, and then all
                            // have the same signing key, even if that's not the
                            // way we are adding them
                            // (the key of the last inserted gets used for all of them).

                            use std::convert::TryInto;
                            let derivation: u32 = fake_derivation
                                .try_into()
                                .expect("can't cast usize to u32 safely");

                            let key = Key::new_unchecked(
                                key,
                                DerivationPath::<AnyScheme>::new()
                                    .append_unchecked(derivation.into()),
                                DerivationScheme::V2,
                            );
                            store = store.add(pointer, key);
                            at_least_one_match = true;
                        }
                    }
                });
            }
        }

        self.state.push(*fragment_id, store);
        at_least_one_match
    }
}