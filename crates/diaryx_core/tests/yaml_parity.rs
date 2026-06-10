//! Read-path parity: the `fig` backend must deserialize Diaryx frontmatter
//! into the same `yaml::Value` tree that `serde_yaml_ng` did. Each case is
//! parsed both ways and the results compared. Cases cover real note frontmatter
//! (quoted link strings, attachment sequences, timestamp scalars) plus the
//! Diaryx-shaped edges (lenient string/array fields, flow/empty collections,
//! quoting that must round-trip as a string).
//!
//! This is a transitional harness; `serde_yaml_ng` is a dev-dependency only.

use diaryx_core::yaml::{self, Value};

/// Parse `src` with both backends and assert the `Value` trees are equal.
fn assert_parity(label: &str, src: &str) {
    let fig: Value = yaml::from_str(src).unwrap_or_else(|e| panic!("[{label}] fig failed: {e}"));
    let ng: Value = serde_yaml_ng::from_str(src)
        .unwrap_or_else(|e| panic!("[{label}] serde_yaml_ng failed: {e}"));
    assert_eq!(
        fig, ng,
        "[{label}] fig vs serde_yaml_ng diverged\nsource:\n{src}"
    );
}

const CASES: &[(&str, &str)] = &[
    (
        "real_attachment_note",
        "title: Audience Filtering Demo\n\
         link: '[Audience Filtering Demo](/_attachments/audience-filter-demo.html.md)'\n\
         attachment: '[audience-filter-demo.html](/_attachments/audience-filter-demo.html)'\n\
         attachment_of:\n\
         - '[Diaryx](/Diaryx.md)'\n\
         updated: 2026-04-08T21:22:52-06:00\n",
    ),
    (
        "scalar_types",
        "s: hello\ni: 42\nf: 3.14\nbig: 10000000\nb_true: true\nb_false: false\n\
         n_null: null\nn_tilde: ~\nempty:\nneg: -5\nzero: 0\n",
    ),
    (
        "quoting_must_stay_string",
        "colon: 'a: b'\nhash: '#tag'\nnum_str: '123'\nbool_str: 'true'\n\
         dquote: \"a quoted line\"\nbrackets: '[link](/x)'\npadded: '  spaced  '\n",
    ),
    (
        "nested_and_flow",
        "meta:\n  author: me\n  revs:\n  - 1\n  - 2\n\
         flow_seq: [a, b, c]\nflow_map: {x: 1, y: 2}\ntags: []\nempty_map: {}\n",
    ),
    (
        "lenient_index_frontmatter",
        "title: Just A String\nversion: 3\ncontents:\n- one\n- two\naudience: public\n",
    ),
    (
        "comments_and_blanks",
        "# leading comment\ntitle: Hi   # trailing comment\n\ncount: 7\n\
         tags:\n  - a   # inline\n  - b\n",
    ),
    (
        "unicode_and_emoji",
        "name: café\nemoji: 🚀\nmixed: 'naïve — value'\n",
    ),
];

#[test]
fn fig_matches_serde_yaml_ng_on_frontmatter_corpus() {
    for (label, src) in CASES {
        assert_parity(label, src);
    }
}

/// Typed-struct round-trips (the `parse_typed`/`from_str::<T>` path Diaryx uses
/// for index/config files) must also agree across backends.
#[test]
fn fig_matches_serde_yaml_ng_for_typed_maps() {
    use std::collections::BTreeMap;
    for (label, src) in CASES {
        let fig: BTreeMap<String, Value> =
            yaml::from_str(src).unwrap_or_else(|e| panic!("[{label}] fig typed failed: {e}"));
        let ng: BTreeMap<String, Value> = serde_yaml_ng::from_str(src)
            .unwrap_or_else(|e| panic!("[{label}] ng typed failed: {e}"));
        assert_eq!(fig, ng, "[{label}] typed map diverged");
    }
}
