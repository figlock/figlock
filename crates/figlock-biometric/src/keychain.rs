//! Creating a login keychain item that any app of the same developer team can read without a prompt (FIG-106).
//!
//! The default access list of a new item trusts only the app that made it, matched by its code identifier, so
//! the desktop app is prompted for an item the command line tool made and the other way round. An item made
//! here has no per-app trust entry ("any application"); macOS still applies the item's partition list, which
//! it fills with the creating app's team (`teamid:<team>`), so an app from another team is prompted as before.
//! An unsigned build has no team: its partition is its own code hash, so only that exact binary reads it.
//!
//! The safe `security-framework` API cannot set an access list, and the sys crate does not declare the two
//! symbols used here, so they are declared below.

#![allow(unsafe_code, reason = "calls the macOS Security framework")]

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::data::CFData;
use core_foundation::dictionary::CFMutableDictionary;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::array::CFArrayRef;
use core_foundation_sys::base::CFRelease;
use security_framework_sys::base::SecAccessRef;
use security_framework_sys::item::{
    kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword, kSecValueData,
};
use security_framework_sys::keychain_item::SecItemAdd;

/// `errSecDuplicateItem`: an item with that service and account exists already.
pub const ERR_SEC_DUPLICATE_ITEM: i32 = -25299;

type SecAclRef = *mut std::ffi::c_void;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecAccessCreate(
        descriptor: CFStringRef,
        trustedlist: *const std::ffi::c_void,
        access_ref: *mut SecAccessRef,
    ) -> i32;
    fn SecAccessCopyMatchingACLList(
        access: SecAccessRef,
        authorization_tag: CFTypeRef,
    ) -> CFArrayRef;
    fn SecACLCopyContents(
        acl: SecAclRef,
        application_list: *mut CFArrayRef,
        description: *mut CFStringRef,
        prompt_selector: *mut u16,
    ) -> i32;
    /// An application list of NULL means any application may use the item without a prompt.
    fn SecACLSetContents(
        acl: SecAclRef,
        application_list: CFArrayRef,
        description: CFStringRef,
        prompt_selector: u16,
    ) -> i32;
    static kSecAttrAccess: CFStringRef;
    static kSecACLAuthorizationDecrypt: CFStringRef;
}

/// A new access object whose read permission trusts any application. `SecAccessCreate` always trusts the
/// calling application, whatever list is passed, so the entry is edited after it is made.
///
/// # Safety
/// Calls the Security framework with objects made here.
unsafe fn any_app_access(descriptor: &CFString) -> Result<CFType, i32> {
    let mut access: SecAccessRef = std::ptr::null_mut();
    // SAFETY: `descriptor` is live and `access` is a valid out pointer.
    let status = unsafe {
        SecAccessCreate(
            descriptor.as_concrete_TypeRef(),
            std::ptr::null(),
            &mut access,
        )
    };
    if status != 0 || access.is_null() {
        return Err(if status != 0 { status } else { -1 });
    }
    // SAFETY: the create rule: this owns one reference, released when the `CFType` drops.
    let access = unsafe { CFType::wrap_under_create_rule(access.cast()) };

    // SAFETY: `access` is a live access object and the tag is a framework constant. The copy rule applies.
    let acls = unsafe {
        SecAccessCopyMatchingACLList(
            access.as_CFTypeRef().cast_mut().cast(),
            kSecACLAuthorizationDecrypt.cast(),
        )
    };
    if acls.is_null() {
        return Err(-1);
    }
    // SAFETY: the create rule for a `Copy` function.
    let acls: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(acls) };
    let mut edited = false;
    for acl in acls.iter() {
        let acl = acl.as_CFTypeRef().cast_mut();
        let mut apps: CFArrayRef = std::ptr::null();
        let mut description: CFStringRef = std::ptr::null();
        let mut selector = 0u16;
        // SAFETY: `acl` is a live ACL entry of `access`; the out pointers are valid.
        let status = unsafe { SecACLCopyContents(acl, &mut apps, &mut description, &mut selector) };
        if status != 0 {
            return Err(status);
        }
        // SAFETY: both were returned under the copy rule; `description` is kept until after the call.
        let status = unsafe {
            if !apps.is_null() {
                CFRelease(apps.cast());
            }
            let status = SecACLSetContents(acl, std::ptr::null(), description, selector);
            if !description.is_null() {
                CFRelease(description.cast());
            }
            status
        };
        if status != 0 {
            return Err(status);
        }
        edited = true;
    }
    if edited { Ok(access) } else { Err(-1) }
}

/// Add a generic password item that any app of this developer team can read. Fails with
/// [`ERR_SEC_DUPLICATE_ITEM`] when the item exists; the caller decides what to do about that. The error is the
/// raw `OSStatus`.
pub fn add_shared_generic_password(service: &str, account: &str, secret: &[u8]) -> Result<(), i32> {
    // SAFETY: every pointer passed is a live Core Foundation object owned by this function, the access object
    // returned by `SecAccessCreate` follows the create rule and is released by `CFType`, and the dictionary
    // keys are framework constants.
    unsafe {
        let access = any_app_access(&CFString::new(service))?;

        let service = CFString::new(service);
        let account = CFString::new(account);
        let data = CFData::from_buffer(secret);
        let mut attrs = CFMutableDictionary::new();
        attrs.add(&kSecClass.cast(), &kSecClassGenericPassword.cast());
        attrs.add(&kSecAttrService.cast(), &service.as_CFTypeRef());
        attrs.add(&kSecAttrAccount.cast(), &account.as_CFTypeRef());
        attrs.add(&kSecValueData.cast(), &data.as_CFTypeRef());
        attrs.add(&kSecAttrAccess.cast(), &access.as_CFTypeRef());

        match SecItemAdd(attrs.as_concrete_TypeRef(), std::ptr::null_mut()) {
            0 => Ok(()),
            status => Err(status),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    /// The access list `security dump-keychain` prints for the item with this account.
    fn access_list(account: &str) -> String {
        let dump = Command::new("/usr/bin/security")
            .args(["dump-keychain", "-a"])
            .output()
            .expect("run security");
        let dump = String::from_utf8_lossy(&dump.stdout).into_owned();
        let block = dump
            .split("keychain: ")
            .find(|b| b.contains(account))
            .expect("the item is in the keychain");
        block[block.find("access:").expect("an access list")..].to_owned()
    }

    fn cleanup(service: &str, account: &str) {
        let _ = Command::new("/usr/bin/security")
            .args(["delete-generic-password", "-s", service, "-a", account])
            .output();
    }

    /// Writes to the real login keychain, so it runs only on request:
    /// `cargo test -p figlock-biometric -- --ignored keychain`
    #[test]
    #[ignore = "writes a throwaway item to the login keychain"]
    fn an_item_has_no_per_app_trust_and_a_duplicate_is_reported() {
        let service = "com.figlock.test.shared-access";
        let account = format!("{:032x}", std::process::id());
        cleanup(service, &account);

        add_shared_generic_password(service, &account, b"test-secret").expect("add");
        assert_eq!(
            add_shared_generic_password(service, &account, b"again"),
            Err(ERR_SEC_DUPLICATE_ITEM)
        );

        let list = access_list(&account);
        cleanup(service, &account);

        // The read permission names no application: any app may use it, and the partition list decides.
        let read = list
            .split("entry ")
            .find(|e| e.contains("decrypt"))
            .expect("a read entry");
        assert!(read.contains("applications: <null>"), "{list}");
        assert!(list.contains("partition_id"), "{list}");
    }
}
