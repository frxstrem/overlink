pub use overlink_macros::overlink;

#[doc(hidden)]
pub mod __reexport {
    pub use core;
    pub use libc;
    pub use std;
}

#[doc(hidden)]
pub mod __internals {
    use core::{ffi::CStr, mem};
    use std::{cell::Cell, thread::LocalKey};

    pub const fn next_symbol_check_types<T: 'static>() {
        assert!(
            mem::size_of::<T>() == mem::size_of::<*mut libc::c_void>()
                && mem::align_of::<T>() == mem::align_of::<*mut libc::c_void>(),
            "T must be a pointer-sized type"
        );
    }

    pub unsafe fn next_symbol<T: 'static>(name: &CStr) -> Result<T, SymbolResolutionError<'_>> {
        next_symbol_check_types::<T>();

        let symbol = libc::dlsym(libc::RTLD_NEXT, name.as_ptr() as *const libc::c_char);

        let error: Option<&'static CStr> = unsafe { dlerror_if_safe() };

        if symbol.is_null() {
            return Err(SymbolResolutionError {
                name,
                message: error.and_then(|error| error.to_str().ok()),
            });
        }

        Ok(mem::transmute_copy(&symbol))
    }

    /// Call `dlerror()` if it is thread safe on the current platform.
    unsafe fn dlerror_if_safe() -> Option<&'static CStr> {
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            // dlerror() is known to be thread-safe on Linux and Mac
            let error = libc::dlerror();

            if !error.is_null() {
                Some(CStr::from_ptr(error))
            } else {
                None
            }
        } else {
            // for platforms where dlerror() may not be thread-safe, we just return
            // `None`
            None
        }
    }

    #[derive(Debug)]
    pub struct SymbolResolutionError<'a> {
        pub name: &'a CStr,
        pub message: Option<&'static str>,
    }

    impl core::fmt::Display for SymbolResolutionError<'_> {
        fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
            let name = self.name;
            if let Some(message) = self.message {
                write!(f, "failed to resolve symbol {name:?}: {message}")
            } else {
                write!(f, "failed to resolve symbol {name:?}")
            }
        }
    }

    impl core::error::Error for SymbolResolutionError<'_> {}

    pub fn guard_recursion(cell: &'static LocalKey<Cell<bool>>) -> Option<RecursionGuard> {
        let old_value = cell.replace(true);

        if !old_value {
            Some(RecursionGuard { cell })
        } else {
            None
        }
    }

    pub struct RecursionGuard {
        cell: &'static LocalKey<Cell<bool>>,
    }

    impl Drop for RecursionGuard {
        fn drop(&mut self) {
            self.cell.set(false);
        }
    }
}
