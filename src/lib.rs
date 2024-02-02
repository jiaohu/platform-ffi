extern crate core;

use anyhow::{anyhow, Result};
use core::slice;
use finutils::txn_builder::TransactionBuilder;
use finutils::txn_builder::TransferOperationBuilder;
use globutils::wallet;
use ledger::data_model::BLACK_HOLE_PUBKEY;
use ledger::data_model::TX_FEE_MIN;
use ledger::data_model::{b64dec, TransferType, TxoRef, TxoSID, Utxo, ASSET_TYPE_FRA};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::c_char;
use zei::serialization::ZeiFromToBytes;
use zei::xfr::asset_record::{open_blind_asset_record, AssetRecordType};
use zei::xfr::sig::XfrPublicKey;
use zei::xfr::structs::{AssetRecordTemplate, OwnerMemo};

#[no_mangle]
pub extern "C" fn add(a: u64, b: u64) -> u64 {
    a + b
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Memo {
    p: String,
    op: String,
    tick: String,
    amt: String,
}

impl Memo {
    fn new(p: String, op: String, tick: String, amt: String) -> Self {
        Self { p, op, tick, amt }
    }
}

#[no_mangle]
pub extern "C" fn get_tx_str(
    from_sig_ptr: *mut u8,
    from_sig_len: u32,
    to_ptr: *mut u8,
    to_len: u32,
    trans_amount: u64,
    url_ptr: *mut u8,
    url_len: u32,
    tick_ptr: *mut u8,
    tick_len: u8,
) -> *const c_char {
    let from_key = unsafe { slice::from_raw_parts(from_sig_ptr, from_sig_len as usize) };
    let to_pub_key = unsafe { slice::from_raw_parts(to_ptr, to_len as usize) };
    let tick = unsafe { slice::from_raw_parts(tick_ptr, tick_len as usize) };
    let url = unsafe { slice::from_raw_parts(url_ptr, url_len as usize) };
    let url_str = std::str::from_utf8(url).unwrap();

    let from_key_str = std::str::from_utf8(from_key).unwrap();
    let from = wallet::restore_keypair_from_mnemonic_default(from_key_str).unwrap();
    let aaaas = b64dec(to_pub_key).unwrap();
    let to = XfrPublicKey::zei_from_bytes(aaaas.as_slice()).unwrap();
    let asset_record_type = AssetRecordType::from_flags(false, false);

    let mut op = TransferOperationBuilder::new();

    // build input
    let mut input_amount = 0;
    let mut t_amout;
    let utxos = get_owned_utxos_x(
        url_str,
        wallet::public_key_to_base64(from.get_pk_ref()).as_str(),
    )
    .unwrap();
    for (sid, (utxo, owner_memo)) in utxos.into_iter() {
        let oar = open_blind_asset_record(&utxo.0.record, &owner_memo, &from).unwrap();
        if oar.asset_type != ASSET_TYPE_FRA {
            continue;
        }
        t_amout = oar.amount;
        input_amount += t_amout;

        if t_amout != 0 {
            op.add_input(TxoRef::Absolute(sid), oar, None, None, t_amout)
                .unwrap();
            if input_amount > TX_FEE_MIN {
                break;
            }
        }
    }

    let memo_struct = Memo::new(
        "brc-20".to_string(),
        "transfer".to_string(),
        std::str::from_utf8(tick).unwrap().to_string(),
        trans_amount.to_string(),
    );
    let memo = serde_json::to_string(&memo_struct).unwrap();
    let template =
        AssetRecordTemplate::with_no_asset_tracing(0, ASSET_TYPE_FRA, asset_record_type, to);

    let template_from = AssetRecordTemplate::with_no_asset_tracing(
        input_amount - TX_FEE_MIN,
        ASSET_TYPE_FRA,
        asset_record_type,
        from.get_pk(),
    );

    let template_fee = AssetRecordTemplate::with_no_asset_tracing(
        TX_FEE_MIN,
        ASSET_TYPE_FRA,
        asset_record_type,
        *BLACK_HOLE_PUBKEY,
    );
    // build operation
    let trans_build = op
        .add_output(&template_fee, None, None, None, None)
        .and_then(|b| b.add_output(&template, None, None, None, Some(memo)))
        .and_then(|b| b.add_output(&template_from, None, None, None, None))
        .and_then(|b| b.create(TransferType::Standard))
        .and_then(|b| b.sign(&from))
        .and_then(|b| b.transaction())
        .unwrap();

    let mut builder = get_transaction_builder(url_str).unwrap();

    let tx: finutils::transaction::BuildTransaction = builder
        .add_operation(trans_build)
        .sign_to_map(&from)
        .clone()
        .take_transaction();

    let tx_str = serde_json::to_string(&tx).unwrap();
    let c_string = CString::new(tx_str).unwrap();
    c_string.into_raw()
}

#[no_mangle]
pub extern "C" fn get_seq_id(url_ptr: *mut u8, url_len: u32) -> u64 {
    let url = unsafe { slice::from_raw_parts(url_ptr, url_len as usize) };
    let url_str = std::str::from_utf8(url).unwrap();
    let result = get_transaction_builder(url_str).unwrap();
    result.get_seq_id()
}

fn get_transaction_builder(url: &str) -> Result<TransactionBuilder> {
    let url = format!("{}/global_state", url);
    attohttpc::get(&url)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|resp| resp.bytes())
        .map_err(|e| anyhow!("{:?}", e))
        .and_then(|bytes| {
            serde_json::from_slice::<(Value, u64, Value)>(&bytes).map_err(|e| anyhow!("{:?}", e))
        })
        .map(|resp| TransactionBuilder::from_seq_id(resp.1))
}

fn get_owned_utxos_x(
    url: &str,
    pubkey: &str,
) -> Result<HashMap<TxoSID, (Utxo, Option<OwnerMemo>)>> {
    let url = format!("{}/owned_utxos/{}", url, pubkey);

    attohttpc::get(url)
        .send()
        .and_then(|resp| resp.bytes())
        .map_err(|e| anyhow! {"{:?}", e})
        .and_then(|b| {
            serde_json::from_slice::<HashMap<TxoSID, (Utxo, Option<OwnerMemo>)>>(&b)
                .map_err(|e| anyhow!("{:?}", e))
        })
}

#[cfg(test)]
mod tests {
    use crate::{add, Memo};

    #[test]
    fn test_1() {
        assert_eq!(3, add(1, 2));
    }

    #[test]
    fn test_memo() {
        let memo_struct = Memo::new(
            "brc-20".to_string(),
            "transfer".to_string(),
            "ordi".to_string(),
            "1000".to_string(),
        );
        let memo = serde_json::to_string(&memo_struct).unwrap();
        println!("{}", memo);
        assert_eq!(
            "{\"p\":\"brc-20\",\"op\":\"transfer\",\"tick\":\"ordi\",\"amt\":\"1000\"}",
            memo
        )
    }
}
