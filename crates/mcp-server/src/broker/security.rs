#[cfg(any(windows, test))]
use std::io;

#[cfg(windows)]
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
#[cfg(windows)]
use widestring::U16CString;
#[cfg(windows)]
use win_security_identifier::{GetCurrentSid, SecurityIdentifier};

#[cfg(windows)]
pub(crate) fn pipe_security_descriptor_for_current_user() -> io::Result<SecurityDescriptor> {
    let sid = current_user_sid_string()?;
    let sddl = pipe_security_sddl_for_user_sid(&sid)?;
    let sddl = U16CString::from_str(&sddl).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("pipe security descriptor contains an interior NUL: {e}"),
        )
    })?;
    SecurityDescriptor::deserialize(sddl.as_ucstr())
}

#[cfg(windows)]
fn current_user_sid_string() -> io::Result<String> {
    SecurityIdentifier::get_current_user_sid()
        .map(|sid| sid.to_string())
        .map_err(|e| io::Error::other(format!("could not read current Windows user SID: {e}")))
}

#[cfg(any(windows, test))]
pub(crate) fn pipe_security_sddl_for_user_sid(sid: &str) -> io::Result<String> {
    validate_sid(sid)?;
    Ok(format!("O:{sid}D:P(A;;GA;;;{sid})"))
}

#[cfg(any(windows, test))]
fn validate_sid(sid: &str) -> io::Result<()> {
    if !sid.starts_with("S-1-") {
        return invalid_sid(sid);
    }
    if sid.trim() != sid {
        return invalid_sid(sid);
    }
    if sid.contains([';', '(', ')']) {
        return invalid_sid(sid);
    }

    let mut parts = sid.split('-');
    let Some("S") = parts.next() else {
        return invalid_sid(sid);
    };
    let Some("1") = parts.next() else {
        return invalid_sid(sid);
    };
    let mut authorities = 0usize;
    for part in parts {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return invalid_sid(sid);
        }
        authorities += 1;
    }
    if authorities < 2 {
        return invalid_sid(sid);
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn invalid_sid<T>(sid: &str) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid Windows SID: {sid}")))
}

#[cfg(test)]
mod tests {
    use super::pipe_security_sddl_for_user_sid;

    #[test]
    fn pipe_security_sddl_allows_only_exact_user_sid() {
        let sddl = pipe_security_sddl_for_user_sid("S-1-5-21-1-2-3-1001").unwrap();

        assert_eq!(sddl, "O:S-1-5-21-1-2-3-1001D:P(A;;GA;;;S-1-5-21-1-2-3-1001)");
        for forbidden in ["WD", "AN", "AU", "BU", "IU", "BA", "SY", "OW", "CO"] {
            assert!(!sddl.contains(forbidden), "SDDL must not contain {forbidden}: {sddl}");
        }
    }

    #[test]
    fn pipe_security_sddl_rejects_invalid_sid_strings() {
        for sid in [
            "",
            " ",
            "S-1-5-21-1-2-3-1001 ",
            "S-1-5-21-1-2-3-1001;WD",
            "S-1-5-21-1-2-3-1001)",
            "S-1-5-21-1-2-3-abc",
            "S-5-21-1-2-3-1001",
            "X-1-5-21-1-2-3-1001",
        ] {
            assert!(pipe_security_sddl_for_user_sid(sid).is_err(), "SID must be rejected: {sid}");
        }
    }
}
