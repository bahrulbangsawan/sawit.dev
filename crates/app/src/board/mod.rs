//! The native canvas tab: a FigCraft document, rendered off the UI thread and painted by GPUI.
//! Only the GPUI side lives here; the document logic is `sawit_board` (`crates/board`).

// Nothing outside the tests uses the canvas until it is opened from the workspace (Plan 1,
// Task 6). Task 6 removes this allow, with anything still unused.
#![allow(dead_code)]

mod worker;
