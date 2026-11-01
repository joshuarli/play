use player_core::abi_version;

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_abi_version() -> u32 {
    abi_version()
}

#[cfg(test)]
mod tests {
    use super::player_ffi_abi_version;

    #[test]
    fn ffi_reports_core_version() {
        assert_eq!(player_ffi_abi_version(), 1);
    }
}
