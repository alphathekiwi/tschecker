use anyhow::Result;
use std::io::Write;

use crate::gitbutler::Branch;

/// Display branches and let user pick one with a single keypress.
/// IDs: 0-9 for first 10, then a-z for 10-35.
pub fn select_branch(branches: &[&Branch]) -> Result<usize> {
    if branches.is_empty() {
        anyhow::bail!("No applied branches found");
    }

    if branches.len() == 1 {
        eprintln!("Auto-selecting only branch: {}", branches[0].name);
        return Ok(0);
    }

    let mut stderr = std::io::stderr();
    writeln!(stderr)?;
    writeln!(stderr, "Select a branch to check:")?;
    writeln!(stderr)?;

    for (i, branch) in branches.iter().enumerate() {
        let id = index_to_char(i);
        writeln!(stderr, "  {} > {} ({})", id, branch.name, branch.cli_id)?;
    }

    writeln!(stderr)?;
    write!(stderr, "Press key (or q to quit): ")?;
    stderr.flush()?;

    let ch = read_single_key()?;
    writeln!(stderr)?;

    if ch == 'q' || ch == 'Q' {
        anyhow::bail!("Cancelled by user");
    }

    let idx = char_to_index(ch)
        .ok_or_else(|| anyhow::anyhow!("Invalid selection: '{}'", ch))?;

    if idx >= branches.len() {
        anyhow::bail!("Selection '{}' out of range (max {})", ch, branches.len() - 1);
    }

    Ok(idx)
}

fn index_to_char(i: usize) -> char {
    if i < 10 {
        char::from_digit(i as u32, 10).unwrap()
    } else {
        (b'a' + (i - 10) as u8) as char
    }
}

fn char_to_index(ch: char) -> Option<usize> {
    if ch.is_ascii_digit() {
        Some(ch.to_digit(10).unwrap() as usize)
    } else if ch.is_ascii_lowercase() {
        Some((ch as u8 - b'a') as usize + 10)
    } else {
        None
    }
}

/// Read a single keypress using raw terminal mode
fn read_single_key() -> Result<char> {
    use std::io::Read;

    let fd = 0; // stdin
    let mut old_termios = std::mem::MaybeUninit::uninit();

    unsafe {
        if libc::tcgetattr(fd, old_termios.as_mut_ptr()) != 0 {
            anyhow::bail!("Failed to get terminal attributes");
        }
    }

    let old_termios = unsafe { old_termios.assume_init() };
    let mut raw = old_termios;

    raw.c_lflag &= !(libc::ICANON | libc::ECHO);
    raw.c_cc[libc::VMIN] = 1;
    raw.c_cc[libc::VTIME] = 0;

    unsafe {
        if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
            anyhow::bail!("Failed to set raw terminal mode");
        }
    }

    let mut buf = [0u8; 1];
    let result = std::io::stdin().read_exact(&mut buf);

    // Always restore terminal
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, &old_termios);
    }

    result?;
    Ok(buf[0] as char)
}
