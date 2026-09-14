use super::protocol::{decode_frame, encode_frame, validate_request, Request, MAX_FRAME};
use std::io::{Read, Write};

#[cfg(windows)]
use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

#[cfg(windows)]
use windows::{
    core::{HRESULT, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, HANDLE},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::{ReadFile, WriteFile},
        Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
        System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
            GetNamedPipeServerProcessId, SetNamedPipeHandleState, PIPE_NOWAIT, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        },
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalPipeName(String);

impl LocalPipeName {
    pub(crate) fn from_nonce(nonce: &str) -> Result<Self, &'static str> {
        if nonce.len() != 64
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("ProtocolRejected");
        }
        Ok(Self(format!(r"\\.\pipe\uipilot-elevation-{nonce}")))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(windows)]
pub(crate) struct LocalPipeServer(HANDLE);

#[cfg(windows)]
pub(crate) struct PipeStream(HANDLE);

#[cfg(windows)]
impl LocalPipeServer {
    pub(crate) fn create(name: &LocalPipeName, sddl: &str) -> Result<Self, &'static str> {
        if sddl.is_empty() || sddl.contains('\0') || sddl.len() > 4096 {
            return Err("ProtocolRejected");
        }
        let name: Vec<u16> = OsStr::new(name.as_str())
            .encode_wide()
            .chain(Some(0))
            .collect();
        let sddl: Vec<u16> = OsStr::new(sddl).encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                1,
                &mut descriptor,
                None,
            )
            .map_err(|_| "PipeSecurityRejected")?;
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            };
            let pipe = CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                0,
                Some(&attributes),
            );
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(descriptor.0),
            ));
            if pipe.is_invalid() {
                return Err("PipeCreationDenied");
            }
            Ok(Self(pipe))
        }
    }
    pub(crate) fn set_nonblocking(&self) -> Result<(), &'static str> {
        unsafe { SetNamedPipeHandleState(self.0, Some(&PIPE_NOWAIT), None, None) }
            .map_err(|_| "PipeModeRejected")
    }
    pub(crate) fn client_pid(&self) -> Result<u32, &'static str> {
        let mut pid = 0;
        unsafe { GetNamedPipeClientProcessId(self.0, &mut pid) }.map_err(|_| "PeerUnavailable")?;
        if pid == 0 {
            return Err("PeerUnavailable");
        }
        Ok(pid)
    }
    pub(crate) fn server_pid(&self) -> Result<u32, &'static str> {
        let mut pid = 0;
        unsafe { GetNamedPipeServerProcessId(self.0, &mut pid) }.map_err(|_| "PeerUnavailable")?;
        if pid == 0 {
            return Err("PeerUnavailable");
        }
        Ok(pid)
    }
    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
    pub(crate) fn connect(&self) -> Result<(), &'static str> {
        match unsafe { ConnectNamedPipe(self.0, None) } {
            Ok(()) => Ok(()),
            Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0) => Ok(()),
            Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_LISTENING.0) => {
                Err("PipeConnectionPending")
            }
            Err(_) => Err("PipeConnectionFailed"),
        }
    }
    pub(crate) fn disconnect(&self) -> Result<(), &'static str> {
        unsafe { DisconnectNamedPipe(self.0) }.map_err(|_| "PipeDisconnectFailed")
    }
    pub(crate) fn into_stream(self) -> PipeStream {
        let handle = self.0;
        std::mem::forget(self);
        PipeStream(handle)
    }
}

#[cfg(windows)]
impl Drop for LocalPipeServer {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
impl Read for PipeStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut count = 0;
        unsafe { ReadFile(self.0, Some(buffer), Some(&mut count), None) }
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe read failed"))?;
        Ok(count as usize)
    }
}

#[cfg(windows)]
impl Write for PipeStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let mut count = 0;
        unsafe { WriteFile(self.0, Some(buffer), Some(&mut count), None) }.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe write failed")
        })?;
        Ok(count as usize)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for PipeStream {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub(crate) fn read_frame<R: Read, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> Result<T, &'static str> {
    let mut header = [0u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|_| "ProtocolRejected")?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME - 4 {
        return Err("ProtocolRejected");
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|_| "ProtocolRejected")?;
    let mut frame = Vec::with_capacity(MAX_FRAME.min(length + 4));
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&payload);
    decode_frame(&frame)
}

pub(crate) fn write_frame<W: Write, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), &'static str> {
    let frame = encode_frame(value)?;
    writer
        .write_all(&frame)
        .map_err(|_| "TransportUnavailable")?;
    writer.flush().map_err(|_| "TransportUnavailable")
}

pub(crate) fn read_validated_request<R: Read>(reader: &mut R) -> Result<Request, &'static str> {
    let request: Request = read_frame(reader)?;
    validate_request(&request)?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_elevation::protocol::RootSelector;
    use crate::process_elevation::{Operation, Request, Scope};

    fn request() -> Request {
        Request {
            version: 1,
            request_id: "a".repeat(64),
            scope: Scope::Single,
            operation: Operation::Terminate,
            root: RootSelector {
                pid: 7,
                creation_time: 11,
            },
        }
    }
    #[test]
    fn bounded_reader_and_writer_round_trip_a_typed_frame() {
        let value = request();
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &value).unwrap();
        assert_eq!(
            read_frame::<_, Request>(&mut bytes.as_slice()).unwrap(),
            value
        );
    }
    #[test]
    fn rejects_declared_payload_before_allocating_it() {
        let header = (MAX_FRAME as u32).to_le_bytes();
        assert_eq!(
            read_frame::<_, Request>(&mut header.as_slice()),
            Err("ProtocolRejected")
        );
    }

    #[test]
    fn pipe_name_is_local_and_nonce_bound() {
        let name = LocalPipeName::from_nonce(&"a".repeat(64)).unwrap();
        assert_eq!(
            name.as_str(),
            r"\\.\pipe\uipilot-elevation-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(LocalPipeName::from_nonce(&"A".repeat(64)).is_err());
        assert!(LocalPipeName::from_nonce(&"a".repeat(63)).is_err());
    }

    #[test]
    fn request_reader_applies_semantic_validation_before_returning() {
        let invalid = Request {
            version: 1,
            request_id: "a".repeat(64),
            scope: Scope::Single,
            operation: Operation::Terminate,
            root: RootSelector {
                pid: 0,
                creation_time: 11,
            },
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &invalid).unwrap();
        assert_eq!(
            read_validated_request(&mut bytes.as_slice()),
            Err("ProtocolRejected")
        );
    }

    #[cfg(windows)]
    #[test]
    fn named_pipe_rejects_empty_or_oversized_security_descriptor_before_creation() {
        let name = LocalPipeName::from_nonce(&"b".repeat(64)).unwrap();
        assert!(matches!(
            LocalPipeServer::create(&name, ""),
            Err("ProtocolRejected")
        ));
        assert!(matches!(
            LocalPipeServer::create(&name, &"x".repeat(4097)),
            Err("ProtocolRejected")
        ));
        assert!(matches!(
            LocalPipeServer::create(&name, "bad\0sddl"),
            Err("ProtocolRejected")
        ));
    }
}
