#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
        ffi::OsString,
        fmt,
        fs::{
            self,
            File,
        },
        io::{
            self,
            prelude::*,
        },
        path::Path,
        process::Command,
    },
    bytesize::ByteSize,
    derive_more::From,
    structopt::StructOpt,
    systemstat::{
        Platform as _,
        System,
    },
    cron_wrapper::ERRORS_DIR,
};

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
    #[structopt(long)]
    no_diskspace_check: bool,
}

#[wheel::main]
fn main(args: Args) -> Result<(), Error> {
    let perm_path = Path::new(ERRORS_DIR).join(format!("cronjob-{}.log", args.name));
    if !args.no_diskspace_check {
        //TODO move part of diskspace to a library crate and use that instead
        let fs = System::new().mount_at("/")?;
        if fs.avail < ByteSize::gib(5) || (fs.avail.as_u64() as f64 / fs.total.as_u64() as f64) < 0.05
        || fs.files_avail < 5000 || (fs.files_avail as f64 / fs.files_total as f64) < 0.05 {
            fs::write(perm_path, b"not enough disk space\n")?;
            return Ok(())
        }
    }
    let tmp_file = tempfile::Builder::new()
        .prefix(&format!("cronjob-{}", args.name))
        .suffix(".log")
        .tempfile()?;
    let status = match Command::new(args.cmd).args(args.args).stdout(tmp_file.reopen()?).status() {
        Ok(status) => status,
        Err(e) => {
            let mut perm_file = File::create(perm_path)?;
            writeln!(perm_file, "error calling cronjob:\n{}\n{:?}", e, e)?;
            return Ok(())
        }
    };
    if status.success() {
        fs::remove_file(perm_path).not_found_ok()?;
    } else {
        fs::rename(tmp_file, perm_path)?;
    }
    Ok(())
}
