//! The browser's side panel modes and what a click on the page does.

/// The side panel's modes; one at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Annotate,
    Design,
}

impl Panel {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Panel::Annotate => "annotate",
            Panel::Design => "design",
        }
    }
}

/// What a click on the page does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickMode {
    Off,
    Annotate,
    Inspect,
}

impl PickMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PickMode::Off => "off",
            PickMode::Annotate => "annotate",
            PickMode::Inspect => "inspect",
        }
    }
}
