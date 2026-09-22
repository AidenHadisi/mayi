//! Confirm window for a tool call the classifier did not allow.
//!
//! On Linux this uses `zenity`, which must be on `PATH`. If no window can be
//! shown, the answer is [`Answer::Deny`].

use std::fmt;

use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

/// The user's pick in the confirmation dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Approve,
    Save,
    Deny,
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Approve => "Approve",
            Self::Save => "Approve and save",
            Self::Deny => "Deny",
        })
    }
}

/// Show `message` and return the pick. Anything other than Approve or Save is [`Answer::Deny`].
pub fn ask(message: &str) -> Answer {
    let result = MessageDialog::new()
        .set_title("mayi")
        .set_description(message)
        .set_level(MessageLevel::Warning)
        .set_buttons(MessageButtons::YesNoCancelCustom(
            Answer::Approve.to_string(),
            Answer::Save.to_string(),
            Answer::Deny.to_string(),
        ))
        .show();

    let MessageDialogResult::Custom(picked) = result else {
        return Answer::Deny;
    };

    match picked {
        l if l == Answer::Approve.to_string() => Answer::Approve,
        l if l == Answer::Save.to_string() => Answer::Save,
        _ => Answer::Deny,
    }
}
