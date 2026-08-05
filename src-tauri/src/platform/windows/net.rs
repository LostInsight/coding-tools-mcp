use std::mem::{size_of, size_of_val};

use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, SetTcpEntry, MIB_TCPROW_OWNER_PID, MIB_TCP_STATE_DELETE_TCB,
    MIB_TCP_STATE_LISTEN, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::AF_INET;

use crate::error::{AppError, AppResult};

/// Layout-compatible with `MIB_TCPROW` for `SetTcpEntry`.
#[repr(C)]
struct TcpRow {
    dw_state: u32,
    dw_local_addr: u32,
    dw_local_port: u32,
    dw_remote_addr: u32,
    dw_remote_port: u32,
}

/// Windows stores TCP ports in network byte order inside `dwLocalPort`.
fn local_port_from_dw(dw_local_port: u32) -> u16 {
    ((dw_local_port >> 8) & 0xFF) as u16 | ((dw_local_port & 0xFF) as u16) << 8
}

#[cfg(test)]
fn port_to_dw(port: u16) -> u32 {
    ((port as u32) << 8) | ((port as u32) >> 8)
}

fn parse_listener_table_words(
    words: &[u32],
    reported_byte_len: usize,
) -> AppResult<Vec<MIB_TCPROW_OWNER_PID>> {
    let allocated_byte_len = size_of_val(words);
    if reported_byte_len > allocated_byte_len {
        return Err(AppError::Message(format!(
            "GetExtendedTcpTable reported {reported_byte_len} bytes for a {allocated_byte_len}-byte buffer"
        )));
    }

    let header_len = size_of::<u32>();
    if reported_byte_len < header_len || words.is_empty() {
        return Err(AppError::Message(
            "GetExtendedTcpTable returned a truncated table header".into(),
        ));
    }

    let row_count = words[0] as usize;
    let rows_byte_len = row_count
        .checked_mul(size_of::<MIB_TCPROW_OWNER_PID>())
        .ok_or_else(|| AppError::Message("GetExtendedTcpTable row count overflow".into()))?;
    let required_byte_len = header_len
        .checked_add(rows_byte_len)
        .ok_or_else(|| AppError::Message("GetExtendedTcpTable size overflow".into()))?;
    if required_byte_len > reported_byte_len {
        return Err(AppError::Message(format!(
            "GetExtendedTcpTable returned a truncated table: {row_count} rows require {required_byte_len} bytes, got {reported_byte_len}"
        )));
    }

    let base = words.as_ptr().cast::<u8>();
    let mut rows = Vec::with_capacity(row_count);
    for index in 0..row_count {
        let offset = header_len + index * size_of::<MIB_TCPROW_OWNER_PID>();
        // Copy rows from the API buffer so a reported length cannot create an
        // out-of-bounds slice.
        rows.push(unsafe {
            base.add(offset)
                .cast::<MIB_TCPROW_OWNER_PID>()
                .read_unaligned()
        });
    }
    Ok(rows)
}

fn listener_table() -> AppResult<Vec<MIB_TCPROW_OWNER_PID>> {
    let mut size = 0u32;
    unsafe {
        let _ = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0.into(),
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }

    if size < size_of::<u32>() as u32 {
        return Err(AppError::Message(format!(
            "GetExtendedTcpTable returned an invalid buffer size: {size}"
        )));
    }

    let word_count = (size as usize).div_ceil(size_of::<u32>());
    let mut buffer = vec![0u32; word_count];
    let mut returned_size = size;
    let status = unsafe {
        GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut returned_size,
            false,
            AF_INET.0.into(),
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != 0 {
        return Err(AppError::Message(format!(
            "GetExtendedTcpTable failed: status={status}"
        )));
    }

    parse_listener_table_words(&buffer, returned_size as usize)
}

fn find_listener_row(port: u16) -> AppResult<Option<MIB_TCPROW_OWNER_PID>> {
    for row in listener_table()? {
        if row.dwState == MIB_TCP_STATE_LISTEN.0 as u32
            && local_port_from_dw(row.dwLocalPort) == port
        {
            return Ok(Some(row));
        }
    }
    Ok(None)
}

pub fn find_pid_listening_on_port(port: u16) -> AppResult<Option<u32>> {
    Ok(find_listener_row(port)?.map(|row| row.dwOwningPid))
}

/// Force-close a TCP listener via `SetTcpEntry(MIB_TCP_STATE_DELETE_TCB)`.
/// Used to reclaim a port still held by this process after a failed graceful stop.
pub fn reclaim_listening_port(port: u16) -> AppResult<bool> {
    let Some(row) = find_listener_row(port)? else {
        return Ok(false);
    };

    let mut tcp_row = TcpRow {
        dw_state: MIB_TCP_STATE_DELETE_TCB.0 as u32,
        dw_local_addr: row.dwLocalAddr,
        dw_local_port: row.dwLocalPort,
        dw_remote_addr: row.dwRemoteAddr,
        dw_remote_port: row.dwRemotePort,
    };

    let status = unsafe { SetTcpEntry((&mut tcp_row as *mut TcpRow).cast()) };
    if status != 0 {
        let message = format!("SetTcpEntry failed for port {port}: Win32 error {status}");
        eprintln!("{message}");
        return Err(AppError::Message(message));
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_port_from_dw_decodes_windows_network_order() {
        assert_eq!(local_port_from_dw(0x901F), 8080);
        assert_eq!(local_port_from_dw(0x5F70), 28767);
        assert_eq!(local_port_from_dw(port_to_dw(8787)), 8787);
    }

    #[test]
    fn listener_table_parser_reads_rows_within_reported_buffer() {
        let words = [
            1,
            MIB_TCP_STATE_LISTEN.0 as u32,
            0,
            port_to_dw(28767),
            0,
            0,
            42,
        ];

        let rows = parse_listener_table_words(&words, size_of_val(&words)).expect("parse");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].dwOwningPid, 42);
        assert_eq!(local_port_from_dw(rows[0].dwLocalPort), 28767);
    }

    #[test]
    fn listener_table_parser_rejects_count_beyond_reported_buffer() {
        let words = [
            2,
            MIB_TCP_STATE_LISTEN.0 as u32,
            0,
            port_to_dw(28767),
            0,
            0,
            42,
        ];

        let error = parse_listener_table_words(&words, size_of_val(&words))
            .expect_err("truncated table must be rejected");

        assert!(error.to_string().contains("truncated"));
    }

    #[test]
    fn listener_table_finds_current_process_socket() {
        let listener =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind listener");
        let port = listener.local_addr().expect("local address").port();

        let pid = find_pid_listening_on_port(port).expect("query listener table");

        assert_eq!(pid, Some(std::process::id()));
    }
}
