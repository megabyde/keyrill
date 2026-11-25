//! Independent mapping decoder child for both runtime feature variants.

#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use keyrill_windows_proof::{
        mapping::{ArmedPage, Availability},
        native::Target,
        options,
        protocol::{Payload, decode_payload},
    };
    use std::io::{Read, Write};
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let [process, started, thread] = arguments.as_slice() else {
        return Err("expected explicit process, creation time and thread".into());
    };
    let target = Target::open(options::number(process).ok_or("invalid process")?)?;
    let mut identity = target.identity;
    identity.thread = options::number(thread).ok_or("invalid thread")?;
    if identity.started != started.parse::<u64>()? || !target.current() {
        return Err("invalid process creation time".into());
    }
    let expected = std::env::var("KEYRILL_DECODER_BUILD_DIGEST")?;
    let expected = options::digest(&expected).ok_or("invalid expected decoder identity")?;
    let page = ArmedPage::create(identity)?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    output.write_all(b"R")?;
    output.flush()?;
    let mut request = [0; 1];
    while input.read(&mut request)? != 0 && request == *b"R" {
        let (availability, words) = page.read();
        if availability != Availability::Available {
            return Err("runtime mapping publication unavailable".into());
        }
        let snapshot =
            decode_payload(&words).map_err(|_| "runtime mapping failed independent decoding")?;
        if snapshot.build != expected
            || snapshot.instance != 1
            || snapshot.service[0] != 7
            || snapshot.service[1] != 1
            || snapshot.service[3] != 2
            || snapshot.service[18] != 1
            || snapshot.service[47..51] != [1, 7, 1, 0x8000_4005]
            || snapshot.lifecycle[..2] != [2, 1]
            || snapshot.lifecycle[12] != 2
            || snapshot.runtime != [7, 2, 2, 1, 0x9abc_def0, 0x1234_5678, 2, 1, 2, 0, 3, 5]
            || snapshot.metadata != [13, 0, 1, 3, 0, 0, 1, 1, 1, 1]
        {
            return Err("runtime mapping differs from the literal SDK fixture".into());
        }
        let mut encoded: Payload = [0; 512];
        encoded[500] = snapshot.architecture;
        encoded[0] = snapshot.instance;
        encoded[1] = snapshot.kind;
        encoded[2..6].copy_from_slice(&snapshot.version);
        encoded[6..14].copy_from_slice(&snapshot.build);
        encoded[14..24].copy_from_slice(&snapshot.metadata);
        encoded[24..79].copy_from_slice(&snapshot.service);
        encoded[79..165].copy_from_slice(&snapshot.lifecycle);
        encoded[165..177].copy_from_slice(&snapshot.runtime);
        encoded[177..500].copy_from_slice(&snapshot.observations);
        if encoded != words {
            return Err("independent mapping roundtrip changed wire bytes".into());
        }
        for word in encoded {
            output.write_all(&word.to_le_bytes())?;
        }
        output.flush()?;
    }
    Ok(())
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
