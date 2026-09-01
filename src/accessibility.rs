//! Accessibility permission detection and the standard macOS prompt
//! (PRD §4). Kept isolated so the rest of the app never touches the raw
//! `AXIsProcessTrusted*` FFI directly.

use accessibility_sys::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt,
};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;

pub const PERMISSION_MESSAGE: &str = "snap needs Accessibility permission.\n\nSystem Settings →\nPrivacy & Security →\nAccessibility";

pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Triggers the standard macOS Accessibility permission prompt if not
/// already trusted. Returns whether the process is trusted afterwards
/// (the prompt itself does not block for the user's answer).
pub fn prompt_for_trust() -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}
