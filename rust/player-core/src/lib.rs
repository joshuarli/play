pub mod command;
pub mod config;
pub mod input;
pub mod local_file;
pub mod playlist;
pub mod state;

pub fn abi_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::abi_version;

    #[test]
    fn abi_version_is_stable() {
        assert_eq!(abi_version(), 1);
    }
}
