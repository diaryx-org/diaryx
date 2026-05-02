#![doc = include_str!(concat!(env!("OUT_DIR"), "/README.md"))]
#![warn(missing_docs)]

pub mod error;
pub mod parser;
pub mod yaml_value;

pub use error::{FrontmatterError, Result};
pub use parser::{
    ParsedFile, extract_body, extract_yaml, get_property, get_string, get_string_array, parse,
    parse_or_empty, parse_typed, remove_property, replace_body, serialize, serialize_typed,
    set_property, sort_alphabetically, sort_by_pattern,
};
pub use yaml_value::{YamlMapping, YamlValue};
