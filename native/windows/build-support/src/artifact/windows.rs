//! Native package I/O refuses reparses during each name lookup and enumerates retained handles.
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_NO_RECALL,
    FILE_SYNCHRONOUS_IO_NONALERT, FileNamesInformation, NtCreateFile, NtQueryDirectoryFile,
};
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::core::PWSTR;

fn invalid() -> io::Error {
    io::Error::other("package native file operation failed")
}

fn native_error(status: NTSTATUS) -> io::Error {
    if status == STATUS_REPARSE_POINT_ENCOUNTERED {
        io::Error::other("package path contains a reparse point")
    } else {
        io::Error::other(format!(
            "package native file operation failed ({:08x})",
            status.0
        ))
    }
}

fn completed(status: NTSTATUS, file: &File, io: *const IO_STATUS_BLOCK) -> Option<NTSTATUS> {
    if status != STATUS_PENDING {
        return Some(status);
    }
    // SAFETY: the caller retains a synchronous file and its complete operation storage.
    if unsafe { WaitForSingleObject(HANDLE(file.as_raw_handle()), INFINITE) } != WAIT_OBJECT_0 {
        return None;
    }
    // SAFETY: the signaled handle establishes completion of this operation's IOSB.
    Some(unsafe { (*io).Anonymous.Status })
}

pub(super) fn open(path: &Path, directory: bool) -> io::Result<File> {
    let path = path.to_str().ok_or_else(invalid)?;
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    if path.len() < 3
        || !path.as_bytes()[0].is_ascii_alphabetic()
        || path.as_bytes()[1..3] != *b":\\"
        || path.encode_utf16().count() > 32760
        || path.contains('\0')
    {
        return Err(invalid());
    }
    struct Open {
        name: Vec<u16>,
        text: UNICODE_STRING,
        attributes: OBJECT_ATTRIBUTES,
        io: IO_STATUS_BLOCK,
        handle: HANDLE,
    }
    let mut operation = Box::new(Open {
        name: format!(r"\??\{path}").encode_utf16().collect(),
        text: UNICODE_STRING::default(),
        attributes: OBJECT_ATTRIBUTES::default(),
        io: IO_STATUS_BLOCK::default(),
        handle: HANDLE::default(),
    });
    operation.text = UNICODE_STRING {
        Length: (operation.name.len() * 2) as u16,
        MaximumLength: (operation.name.len() * 2) as u16,
        Buffer: PWSTR(operation.name.as_mut_ptr()),
    };
    operation.attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        ObjectName: &operation.text,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..Default::default()
    };
    let access = if directory {
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE
    } else {
        FILE_GENERIC_READ
    };
    let kind = if directory {
        FILE_DIRECTORY_FILE
    } else {
        FILE_NON_DIRECTORY_FILE
    };
    // SAFETY: boxed name, attributes, IOSB and output handle survive synchronous completion.
    let status = unsafe {
        NtCreateFile(
            &mut operation.handle,
            access,
            &operation.attributes,
            &mut operation.io,
            None,
            FILE_FLAGS_AND_ATTRIBUTES::default(),
            FILE_SHARE_READ,
            FILE_OPEN,
            kind | FILE_SYNCHRONOUS_IO_NONALERT
                | if directory {
                    Default::default()
                } else {
                    FILE_OPEN_NO_RECALL
                },
            None,
            0,
        )
    };
    if operation.handle.is_invalid() {
        if status == STATUS_PENDING {
            std::mem::forget(operation);
        }
        return Err(native_error(status));
    }
    // SAFETY: a successful acquisition transfers this unique native handle to File.
    let file = unsafe { File::from_raw_handle(operation.handle.0) };
    let Some(status) = completed(status, &file, &raw const operation.io) else {
        // Failed waiting supplies no completion boundary for native output storage.
        std::mem::forget(operation);
        std::mem::forget(file);
        return Err(invalid());
    };
    if status != STATUS_SUCCESS {
        return Err(native_error(status));
    }
    Ok(file)
}

fn entry(bytes: &[u8]) -> io::Result<String> {
    let header = bytes.get(..12).ok_or_else(invalid)?;
    let next = u32::from_le_bytes(header[..4].try_into().map_err(|_| invalid())?);
    let length = u32::from_le_bytes(header[8..12].try_into().map_err(|_| invalid())?) as usize;
    if next != 0 || length == 0 || length > 255 * 2 || !length.is_multiple_of(2) {
        return Err(invalid());
    }
    let name = bytes.get(12..12 + length).ok_or_else(invalid)?;
    let wide: Vec<_> = name
        .as_chunks::<2>()
        .0
        .iter()
        .map(|unit| u16::from_le_bytes(*unit))
        .collect();
    let name = String::from_utf16(&wide).map_err(|_| invalid())?;
    if name.contains(['\0', '/', '\\', ':']) {
        return Err(invalid());
    }
    Ok(name)
}

pub(super) fn entries(file: &File, limit: usize) -> io::Result<Vec<String>> {
    struct Query {
        file: File,
        io: IO_STATUS_BLOCK,
        buffer: [u64; 512],
    }
    let mut operation = Box::new(Query {
        file: file.try_clone()?,
        io: IO_STATUS_BLOCK::default(),
        buffer: [0; 512],
    });
    let mut names = Vec::new();
    let mut restart = true;
    let mut returned = 0;
    loop {
        operation.io = IO_STATUS_BLOCK::default();
        // SAFETY: one-entry query uses an aligned fixed buffer and retained synchronous handle.
        let status = unsafe {
            NtQueryDirectoryFile(
                HANDLE(operation.file.as_raw_handle()),
                None,
                None,
                None,
                &mut operation.io,
                operation.buffer.as_mut_ptr().cast(),
                size_of_val(&operation.buffer) as u32,
                FileNamesInformation,
                true,
                None,
                restart,
            )
        };
        let Some(status) = completed(status, &operation.file, &raw const operation.io) else {
            std::mem::forget(operation);
            return Err(invalid());
        };
        if status == STATUS_NO_MORE_FILES {
            return Ok(names);
        }
        if status != STATUS_SUCCESS || operation.io.Information > size_of_val(&operation.buffer) {
            return Err(invalid());
        }
        returned += 1;
        if returned > limit + 2 {
            return Err(io::Error::other("package contains too many entries"));
        }
        // SAFETY: completion initialized at most Information bytes of this aligned native buffer.
        let bytes = unsafe {
            std::slice::from_raw_parts(operation.buffer.as_ptr().cast(), operation.io.Information)
        };
        let name = entry(bytes)?;
        if name != "." && name != ".." {
            if names.len() == limit {
                return Err(io::Error::other("package contains too many entries"));
            }
            names.push(name);
        }
        restart = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn converted_directory_enumeration_stays_on_its_retained_handle() {
        let source = crate::artifact::tests::Fixture::new();
        let target = crate::artifact::tests::Fixture::new();
        target.candidate();
        let directory = open(&source.0, true).unwrap();
        create_junction(&source.0, &target.0).unwrap();
        assert!(entries(&directory, 512).unwrap().is_empty());
        assert!(open(&source.0.join("package.json"), false).is_err());
        assert!(open(&target.0.join("package.json"), false).is_ok());
        drop(directory);
        std::fs::remove_dir(&source.0).unwrap();
        std::fs::create_dir(&source.0).unwrap();
    }

    fn create_junction(path: &Path, target: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let file = OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES.0)
            .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
            .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0)
            .open(path)?;
        let substitute = format!(r"\??\{}", target.display())
            .encode_utf16()
            .collect::<Vec<_>>();
        let printable = target.to_str().unwrap().encode_utf16().collect::<Vec<_>>();
        let substitute_length = (substitute.len() * 2) as u16;
        let printable_length = (printable.len() * 2) as u16;
        let mut packet = Vec::new();
        packet.extend(0xa0000003u32.to_le_bytes());
        for value in [
            8 + substitute_length + printable_length + 4,
            0,
            0,
            substitute_length,
            substitute_length + 2,
            printable_length,
        ] {
            packet.extend(value.to_le_bytes());
        }
        for unit in substitute
            .into_iter()
            .chain(Some(0))
            .chain(printable)
            .chain(Some(0))
        {
            packet.extend(unit.to_le_bytes());
        }
        let mut returned = 0;
        // SAFETY: the complete mount-point packet changes only this test's owned empty directory.
        unsafe {
            windows::Win32::System::IO::DeviceIoControl(
                HANDLE(file.as_raw_handle()),
                0x900a4,
                Some(packet.as_ptr().cast()),
                packet.len() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )?;
        }
        Ok(())
    }

    #[test]
    fn opens_ordinary_local_ancestry_and_enumerates_owned_directory() {
        let fixture = crate::artifact::tests::Fixture::new();
        fixture.candidate();
        let mut ancestors: Vec<_> = fixture.0.ancestors().collect();
        ancestors.reverse();
        for ancestor in ancestors {
            open(ancestor, true).unwrap();
        }
        let directory = open(&fixture.0, true).unwrap();
        let names = entries(&directory, 512).unwrap();
        assert!(names.contains(&"package.json".to_owned()));
        assert!(entries(&directory, 0).is_err());
    }

    #[test]
    fn rejects_malformed_single_directory_entries() {
        let mut bytes = vec![0u8; 16];
        bytes[8..12].copy_from_slice(&4u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&[b'o', 0, b'k', 0]);
        assert_eq!(entry(&bytes).unwrap(), "ok");
        for length in 0..16 {
            assert!(entry(&bytes[..length]).is_err());
        }
        for (offset, value) in [(0, 4), (8, 0), (8, 3), (8, 512), (8, u32::MAX)] {
            let mut changed = bytes.clone();
            changed[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(entry(&changed).is_err());
        }
        for unit in [0, b'/' as u16, b'\\' as u16, b':' as u16, 0xd800] {
            let mut changed = bytes.clone();
            changed[12..14].copy_from_slice(&unit.to_le_bytes());
            assert!(entry(&changed).is_err());
        }
        let mut longest = vec![0; 12 + 255 * 2];
        longest[8..12].copy_from_slice(&510u32.to_le_bytes());
        for unit in longest[12..].as_chunks_mut::<2>().0 {
            unit.copy_from_slice(&u16::from(b'x').to_le_bytes());
        }
        assert_eq!(entry(&longest).unwrap().len(), 255);
    }
}
