//! Console variables.

use std::{
    cell::UnsafeCell,
    ffi::{c_void, CStr},
    ptr::null_mut,
};

use super::{Module, MODULES};
use crate::{engine, ffi::cvar as ffi, utils::MainThreadMarker};

/// Console variable.
#[derive(Debug)]
pub struct CVar {
    /// The variable itself, linked into the engine cvar list.
    ///
    /// Invariant: `name` and `string` pointers are valid.
    /// Invariant: when `string` is pointing to `name.as_ptr()`, the cvar isn't registered.
    /// Invariant: this is not moved out of while the variable is registered.
    ///
    /// Do not call any engine functions while a reference into the registered `ffi::cvar_s` is
    /// active. Assume any engine function can end up modifying its contents.
    raw: UnsafeCell<ffi::cvar_s>,
    /// Storage for the name.
    name: &'static [u8],
    /// Storage for the default value.
    default_value: &'static [u8],
}

// Safety: all methods accessing `cvar` require a `MainThreadMarker`.
unsafe impl Sync for CVar {}

impl CVar {
    /// Creates a new variable.
    pub const fn new(name: &'static [u8], default_value: &'static [u8]) -> Self {
        Self {
            raw: UnsafeCell::new(ffi::cvar_s {
                name: name.as_ptr().cast(),
                string: default_value.as_ptr().cast(),
                flags: 0,
                value: 0.,
                next: null_mut(),
            }),
            name,
            default_value,
        }
    }

    /// Returns `true` if the variable is currently registered in the engine.
    fn is_registered(&self, _marker: MainThreadMarker) -> bool {
        // Safety: we're not calling any engine methods while the reference is active.
        let raw = unsafe { &*self.raw.get() };

        raw.string != self.default_value.as_ptr().cast()
    }

    /// Returns the `bool` value of the variable.
    ///
    /// # Panics
    ///
    /// Panics if the variable is not registered.
    pub fn as_bool(&self, marker: MainThreadMarker) -> bool {
        assert!(self.is_registered(marker));

        // Safety: we're not calling any engine methods while the reference is active.
        let raw = unsafe { &*self.raw.get() };

        raw.value != 0.
    }
}

/// Registers the variable in the engine.
///
/// As part of the registration the engine will store a pointer to the `raw` field of `cvar`, hence
/// `cvar` must not move after the registration, which is enforced by the 'static lifetime and not
/// having any interior mutability in the public interface.
///
/// # Safety
///
/// This function must only be called when it's safe to register console variables.
///
/// # Panics
///
/// Panics if the variable is already registered.
unsafe fn register(marker: MainThreadMarker, cvar: &'static CVar) {
    assert!(!cvar.is_registered(marker));

    // Make sure the provided name and value are valid C strings.
    assert!(CStr::from_bytes_with_nul(cvar.name).is_ok());
    assert!(CStr::from_bytes_with_nul(cvar.default_value).is_ok());

    engine::CVAR_REGISTERVARIABLE.get(marker)(cvar.raw.get());
}

/// Marks this variable as not registered.
///
/// # Safety
///
/// This function must only be called when the engine does not contain any references to the
/// variable.
unsafe fn mark_as_not_registered(_marker: MainThreadMarker, cvar: &CVar) {
    // Safety: we're not calling any engine methods while the reference is active.
    let raw = &mut *cvar.raw.get();

    raw.string = cvar.default_value.as_ptr().cast();
}

/// De-registers the variable.
///
/// # Safety
///
/// This function must only be called when it's safe to de-register console variables.
///
/// # Panics
///
/// Panics if the variable is not registered.
unsafe fn deregister(marker: MainThreadMarker, cvar: &CVar) {
    assert!(cvar.is_registered(marker));

    // Find a pointer to `cvar`. Start from `cvar_vars` (which points to the first registered
    // variable). On each iteration, check if the pointer points to `cvar`, and if not, follow it.
    // `cvar_vars` can't be null because there's at least one registered variable (the one we're
    // de-registering).
    let mut prev_ptr = engine::CVAR_VARS.get(marker).as_ptr();

    while *prev_ptr != cvar.raw.get() {
        // The next pointer can't be null because we still haven't found our (registered) variable.
        assert!(!(**prev_ptr).next.is_null());

        prev_ptr = &mut (**prev_ptr).next;
    }

    // Make it point to the variable after `cvar`. If there are no variables after `cvar`, it will
    // be set to null as it should be.
    *prev_ptr = (**prev_ptr).next;

    // Free the engine-allocated string and mark the variable as not registered.
    engine::Z_FREE.get(marker)((*cvar.raw.get()).string as *mut c_void);
    mark_as_not_registered(marker, cvar);
}

/// # Safety
///
/// This function must only be called right after `Memory_Init()` completes.
pub unsafe fn register_all_cvars(marker: MainThreadMarker) {
    if !CVars.is_enabled(marker) {
        return;
    }

    for module in MODULES {
        for cvar in module.cvars() {
            trace!(
                "registering {}",
                CStr::from_bytes_with_nul(cvar.name)
                    .unwrap()
                    .to_string_lossy()
            );

            register(marker, cvar);
        }
    }
}

/// # Safety
///
/// This function must only be called right after `Host_Shutdown()` is called.
pub unsafe fn mark_all_cvars_as_not_registered(marker: MainThreadMarker) {
    if !CVars.is_enabled(marker) {
        return;
    }

    for module in MODULES {
        for cvar in module.cvars() {
            // Safety: at this point the engine has no references into the variables and the memory for
            // the variable values is about to be freed.
            mark_as_not_registered(marker, cvar);
        }
    }
}

/// # Safety
///
/// This function must only be called when it's safe to de-register console variables.
pub unsafe fn deregister_disabled_module_cvars(marker: MainThreadMarker) {
    if !CVars.is_enabled(marker) {
        return;
    }

    for module in MODULES {
        if module.is_enabled(marker) {
            continue;
        }

        for cvar in module.cvars() {
            trace!(
                "de-registering {}",
                CStr::from_bytes_with_nul(cvar.name)
                    .unwrap()
                    .to_string_lossy()
            );

            deregister(marker, cvar);
        }
    }
}

pub struct CVars;
impl Module for CVars {
    fn name(&self) -> &'static str {
        "Console variables"
    }

    fn is_enabled(&self, marker: MainThreadMarker) -> bool {
        engine::MEMORY_INIT.is_set(marker)
            && engine::HOST_SHUTDOWN.is_set(marker)
            && engine::CVAR_REGISTERVARIABLE.is_set(marker)
            && engine::Z_FREE.is_set(marker)
            && engine::CVAR_VARS.is_set(marker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cvar_names_and_values() {
        for module in MODULES {
            for cvar in module.cvars() {
                assert!(CStr::from_bytes_with_nul(cvar.name).is_ok());
                assert!(CStr::from_bytes_with_nul(cvar.default_value).is_ok());
            }
        }
    }
}