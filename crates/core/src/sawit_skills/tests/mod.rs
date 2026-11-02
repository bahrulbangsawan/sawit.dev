//! Sawit Skills' tests: the block as text, a file written and read back, each agent's file, and
//! what the footer's button says.

mod blocks;
mod button;
mod paths;
mod projects;
mod writes;

use super::*;

/// This Sawit's block, its lines ending in `\n`.
fn ours() -> String {
    block_text()
}
