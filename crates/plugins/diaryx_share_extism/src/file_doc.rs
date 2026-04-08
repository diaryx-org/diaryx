//! File document CRDT operations using yrs XmlFragment.
//!
//! Each shared file is represented as a yrs Doc containing an XmlFragment.
//! XmlFragment preserves richer structure for collaborative editing with
//! TipTap/ProseMirror. Initial content is inserted as a text node; the
//! editor restructures it into proper AST nodes as users edit.

use yrs::updates::decoder::Decode;
use yrs::{Doc, GetString, ReadTxn, StateVector, Transact, Update, XmlFragment, XmlOut};

const FILE_ROOT: &str = "content";

/// Create a new file doc with initial text content.
pub fn create_file_doc(content: &str) -> Doc {
    let doc = Doc::new();
    {
        let fragment = doc.get_or_insert_xml_fragment(FILE_ROOT);
        let mut txn = doc.transact_mut();
        fragment.insert(&mut txn, 0, yrs::XmlTextPrelim::new(content));
    }
    doc
}

/// Extract the text content from a file doc.
pub fn read_file_doc(doc: &Doc) -> String {
    let fragment = doc.get_or_insert_xml_fragment(FILE_ROOT);
    let txn = doc.transact();
    let mut result = String::new();
    for child in fragment.children(&txn) {
        if let XmlOut::Text(text) = child {
            result.push_str(&text.get_string(&txn));
        }
    }
    result
}

/// Encode the full document state as a yrs update.
pub fn encode_full_state(doc: &Doc) -> Vec<u8> {
    let txn = doc.transact();
    txn.encode_state_as_update_v1(&StateVector::default())
}

/// Apply a remote yrs update to the file doc.
pub fn apply_update(doc: &Doc, update: &[u8]) {
    if let Ok(update) = Update::decode_v1(update) {
        let mut txn = doc.transact_mut();
        let _ = txn.apply_update(update);
    }
}
