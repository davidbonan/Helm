//! What a page waits on with `alert`, `confirm` or `prompt` (browser.md §12).

#[derive(Debug, Clone, PartialEq)]
pub struct PageDialog {
    pub kind: DialogKind,
    pub message: String,
    /// The site that asks (`localhost:3000`), as the card names it.
    pub site: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DialogKind {
    Alert,
    Confirm,
    Prompt { default: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DialogAnswer {
    /// OK; the text is the prompt's field, ignored by `alert` and `confirm`.
    Accept(String),
    Cancel,
}
