#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
        ffi::OsString,
        fmt,
        fs,
        io,
        path::Path,
        process::Command,
    },
    derive_more::From,
    structopt::StructOpt,
};

#[cfg(target_os = "linux")] const ERRORS_DIR: &str = "/home/fenhl/.local/share/syncbin";
#[cfg(target_os = "macos")] const ERRORS_DIR: &str = "/Users/fenhl/Desktop";

trait IoResultExt {
    fn not_found_ok(self) -> Self;
}

impl IoResultExt for io::Result<()> {
    fn not_found_ok(self) -> Self {
        match self {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            x => x,
        }
    }
}

#[derive(From)]
enum Error {
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

#[derive(StructOpt)]
struct Args {
    name: String,
    #[structopt(parse(from_os_str))]
    cmd: OsString,
    #[structopt(parse(from_os_str))]
    args: Vec<OsString>,
}

#[wheel::main]
fn main(args: Args) -> Result<(), Error> {
    let tmp_file = tempfile::Builder::new()
        .prefix(&format!("cronjob-{}", args.name))
        .suffix(".log")
        .tempfile()?;
    let perm_path = Path::new(ERRORS_DIR).join(format!("cronjob-{}.log", args.name));
    if Command::new(args.cmd).args(args.args).stdout(tmp_file.reopen()?).status()?.success() {
        fs::remove_file(perm_path).not_found_ok()?;
    } else {
        fs::rename(tmp_file, perm_path)?;
    }
    Ok(())
}
