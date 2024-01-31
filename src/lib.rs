extern crate core;

use core::slice;
use std::ffi::CString;
use zei::serialization::ZeiFromToBytes;
use zei::xfr::sig::{XfrSecretKey, XfrPublicKey};
use ledger::data_model::{ASSET_TYPE_FRA, TxoRef, TxoSID};
use finutils::{
    txn_builder::TransferOperationBuilder
};
use zei::xfr::asset_record::{AssetRecordType, open_blind_asset_record};
use zei::xfr::structs::{AssetRecordTemplate, BlindAssetRecord};
use anyhow::anyhow;
use finutils::txn_builder::TransactionBuilder;
use serde::{Deserialize, Serialize};
use std::os::raw::c_char;

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
        Self {
            p,
            op,
            tick,
            amt,
        }
    }
}

#[no_mangle]
pub extern "C" fn send(
    from_sig_ptr: *mut u8, from_sig_len: u32,
    to_ptr: *mut u8, to_len: u32,
    txosid: u64,
    assert_record_ptr: *mut u8, assert_record_len: u8,
    input_amount: u64,
    trans_amount: u64,
    seq_id: u64,
    tick_ptr: *mut u8,
    tick_len: u8,
) -> *const c_char {
    let from_key = unsafe {
        slice::from_raw_parts(from_sig_ptr, from_sig_len as usize)
    };
    let to_pub_key = unsafe {
        slice::from_raw_parts(to_ptr, to_len as usize)
    };
    let assert_record = unsafe {
        slice::from_raw_parts(assert_record_ptr, assert_record_len as usize)
    };
    let tick = unsafe {
        slice::from_raw_parts(tick_ptr, tick_len as usize)
    };
    let txo_sid = TxoRef::Absolute(TxoSID(txosid));
    let asset_record = serde_json::from_slice::<BlindAssetRecord>(assert_record).unwrap();

    let from = XfrSecretKey::zei_from_bytes(from_key).unwrap().into_keypair();
    let to = XfrPublicKey::zei_from_bytes(to_pub_key).unwrap();
    let oar = open_blind_asset_record(&asset_record, &None, &from)
        .map_err(|e| anyhow!("Could not open asset record: {}", e)).unwrap();
    // let code = AssetTypeCode {
    //     val: ASSET_TYPE_FRA,
    // }.to_base64();
    let asset_record_type = AssetRecordType::from_flags(false, false);

    // TODO check gas fee to from or BLACK_HOLE_PUBKEY
    let template_fee = AssetRecordTemplate::with_no_asset_tracing(
        input_amount - trans_amount, ASSET_TYPE_FRA, asset_record_type, from.get_pk(),
    );


    let memo_struct = Memo::new("brc-20".to_string(), "transfer".to_string(), std::str::from_utf8(tick).unwrap().to_string(), input_amount.to_string());
    let memo = serde_json::to_string(&memo_struct).unwrap();
    let template = AssetRecordTemplate::with_no_asset_tracing(trans_amount, ASSET_TYPE_FRA, asset_record_type, to);

    let op = TransferOperationBuilder::new()
        .add_input(txo_sid, oar, None, None, input_amount)
        .and_then(|b| b.add_output(&template_fee, None, None, None, None))
        .and_then(|b| b.add_output(&template, None, None, None, Some(memo)))
        .and_then(|b| b.transaction())
        .unwrap();

    let tx = TransactionBuilder::from_seq_id(seq_id).add_operation(op).sign_to_map(&from).clone().take_transaction();

    let tx_str = serde_json::to_string(&tx).unwrap();
    let c_string = CString::new(tx_str).unwrap();
    c_string.as_ptr() as *const c_char
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
        let memo_struct = Memo::new("brc-20".to_string(), "transfer".to_string(), "ordi".to_string(), "1000".to_string());
        let memo = serde_json::to_string(&memo_struct).unwrap();
        println!("{}", memo);
        assert_eq!("{\"p\":\"brc-20\",\"op\":\"transfer\",\"tick\":\"ordi\",\"amt\":\"1000\"}", memo)
    }
}