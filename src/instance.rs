#[cfg(windows)]
mod platform {
    use std::iter;
    use windows::{
        Win32::{
            Foundation::{CloseHandle, GetLastError, HANDLE, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
        },
        core::PCWSTR,
    };

    pub struct InstanceGuard(HANDLE);

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub fn acquire(name: &str) -> Result<Option<InstanceGuard>, String> {
        let name: Vec<u16> = name.encode_utf16().chain(iter::once(0)).collect();
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
            .map_err(|error| format!("Could not create the Moneta instance guard: {error}"))?;

        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Ok(None);
        }

        Ok(Some(InstanceGuard(handle)))
    }
}

#[cfg(not(windows))]
mod platform {
    pub struct InstanceGuard;

    pub fn acquire(_name: &str) -> Result<Option<InstanceGuard>, String> {
        Ok(Some(InstanceGuard))
    }
}

pub use platform::InstanceGuard;

pub fn acquire() -> Result<Option<InstanceGuard>, String> {
    platform::acquire("Local\\WhiteDeath1729.Moneta.SingleInstance")
}

#[cfg(all(test, windows))]
mod tests {
    use super::platform;

    #[test]
    fn only_one_process_can_acquire_the_same_instance_name() {
        let name = format!(
            "Local\\WhiteDeath1729.Moneta.Test.{}",
            std::process::id()
        );

        let first = platform::acquire(&name).expect("first acquire should succeed");
        assert!(first.is_some());

        let second = platform::acquire(&name).expect("second acquire should not error");
        assert!(second.is_none());

        drop(first);
        let third = platform::acquire(&name).expect("mutex should be released on drop");
        assert!(third.is_some());
    }
}
