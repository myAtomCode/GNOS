use crate::fixed::InlineString;
use crate::linux::{Errno, Result};

/// Canonicalize an absolute Linux path without allocating.
pub fn normalize_absolute<'a, const N: usize>(
    path: &str,
    output: &'a mut InlineString<N>,
) -> Result<&'a str> {
    if !path.starts_with('/') {
        return Err(Errno::Invalid);
    }

    output.clear();
    output.push_byte(b'/').map_err(|_| Errno::NameTooLong)?;

    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => pop_component(output),
            component => {
                if output.len() > 1 {
                    output.push_byte(b'/').map_err(|_| Errno::NameTooLong)?;
                }
                output.push_str(component).map_err(|_| Errno::NameTooLong)?;
            }
        }
    }
    Ok(output.as_str())
}

pub fn parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(index) => &path[..index],
    }
}

pub fn normalize_in_root<'a, const N: usize>(
    root: &str,
    cwd: &str,
    input: &str,
    output: &'a mut InlineString<N>,
) -> Result<&'a str> {
    let mut anchor = InlineString::<N>::new();
    normalize_absolute(root, &mut anchor)?;
    let mut base = InlineString::<N>::new();
    normalize_absolute(cwd, &mut base)?;
    let anchored = anchor.as_str() == "/"
        || base.as_str() == anchor.as_str()
        || base
            .as_str()
            .strip_prefix(anchor.as_str())
            .is_some_and(|suffix| suffix.starts_with('/'));
    if !anchored {
        base = anchor;
    }
    output.set(if input.starts_with('/') {
        anchor.as_str()
    } else {
        base.as_str()
    });
    for component in input.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                if output.as_str() != anchor.as_str() {
                    pop_component(output);
                }
            }
            component => {
                if output.len() > 1 {
                    output.push_byte(b'/').map_err(|_| Errno::NameTooLong)?;
                }
                output.push_str(component).map_err(|_| Errno::NameTooLong)?;
            }
        }
    }
    Ok(output.as_str())
}

fn pop_component<const N: usize>(path: &mut InlineString<N>) {
    if path.len() <= 1 {
        return;
    }
    let bytes = path.as_str().as_bytes();
    let new_len = bytes[..path.len()]
        .iter()
        .rposition(|byte| *byte == b'/')
        .unwrap_or(0)
        .max(1);
    path.truncate(new_len);
}
