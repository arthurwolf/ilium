//! Cross-reference checks over the plugin manual: error ids, requirement ids and grant names
//! named in prose must exist in the page that defines them, and the code constants of
//! `ilium-plugin-api/src/codes.rs` must match the error tables (read as text, no compile
//! dependency). There is no requirement id for manual consistency, so these tests carry no
//! `REQ:` tag. Every test lists all of its violations in the failure message.

mod catalogue;
mod ids;
mod manual;
