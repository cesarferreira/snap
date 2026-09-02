//! Safe wrappers over the raw `AXUIElement` C API, kept to the subset snap
//! actually needs.
//!
//! Stands in for the `accessibility` crate: its only reason to depend on the
//! deprecated `cocoa`/`objc` stack — and through it on `block 0.1`, whose
//! `static _NSConcreteStackBlock: Class` is a future hard error — is a
//! bundle-identifier lookup snap never calls.

use std::ffi::c_uchar;

use accessibility_sys::{
    AXError, AXUIElementCopyAttributeValue, AXUIElementCreateApplication, AXUIElementGetTypeID,
    AXUIElementIsAttributeSettable, AXUIElementPerformAction, AXUIElementRef,
    AXUIElementSetAttributeValue, kAXErrorIllegalArgument, kAXErrorNoValue, kAXErrorSuccess, pid_t,
};
use core_foundation::base::{CFType, TCFType, TCFTypeRef};
use core_foundation::string::CFString;
use core_foundation::{declare_TCFType, impl_TCFType};
use core_foundation_sys::base::CFTypeRef;

declare_TCFType!(AXUIElement, AXUIElementRef);
impl_TCFType!(AXUIElement, AXUIElementRef, AXUIElementGetTypeID);

impl AXUIElement {
    pub fn application(pid: pid_t) -> Self {
        unsafe { Self::wrap_under_create_rule(AXUIElementCreateApplication(pid)) }
    }

    /// The attribute's value as an untyped `CFType`. Use [`Self::attribute`]
    /// instead unless the expected type has no `TCFType` impl to check
    /// against — `AXValue`, as used by `AXPosition`/`AXSize`, is the only
    /// such case here.
    pub fn attribute_value(&self, name: &CFString) -> Result<CFType, AXError> {
        let mut value: CFTypeRef = std::ptr::null();
        ax_result(unsafe {
            AXUIElementCopyAttributeValue(self.0, name.as_concrete_TypeRef(), &mut value)
        })?;
        if value.is_null() {
            return Err(kAXErrorNoValue);
        }
        Ok(unsafe { CFType::wrap_under_create_rule(value) })
    }

    /// The attribute's value, or `kAXErrorIllegalArgument` if it isn't a `T`.
    /// AX attributes are dynamically typed, so the check is what keeps a
    /// wrong-typed object from being handed to `T`'s CoreFoundation calls.
    pub fn attribute<T: TCFType>(&self, name: &CFString) -> Result<T, AXError> {
        let value = self.attribute_value(name)?;
        if !value.instance_of::<T>() {
            return Err(kAXErrorIllegalArgument);
        }
        let reference = unsafe { T::Ref::from_void_ptr(value.as_CFTypeRef()) };
        Ok(unsafe { T::wrap_under_get_rule(reference) })
    }

    pub fn set_attribute(&self, name: &CFString, value: &CFType) -> Result<(), AXError> {
        ax_result(unsafe {
            AXUIElementSetAttributeValue(self.0, name.as_concrete_TypeRef(), value.as_CFTypeRef())
        })
    }

    pub fn is_settable(&self, name: &CFString) -> Result<bool, AXError> {
        let mut settable: c_uchar = 0;
        ax_result(unsafe {
            AXUIElementIsAttributeSettable(self.0, name.as_concrete_TypeRef(), &mut settable)
        })?;
        Ok(settable != 0)
    }

    pub fn perform_action(&self, name: &CFString) -> Result<(), AXError> {
        ax_result(unsafe { AXUIElementPerformAction(self.0, name.as_concrete_TypeRef()) })
    }
}

fn ax_result(error: AXError) -> Result<(), AXError> {
    if error == kAXErrorSuccess {
        Ok(())
    } else {
        Err(error)
    }
}
