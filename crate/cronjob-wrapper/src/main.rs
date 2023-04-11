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
        process::{
            Command,
            Stdio,
        },
    },
    bytesize::ByteSize,
    chrono::prelude::*,
    derive_more::From,
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

#[derive(clap::Parser)]
struct Args {
    name: String,
    cmd: OsString,
    args: Vec<OsString>,
    #[clap(long)]
    no_diskspace_check: bool,
}

#[wheel::main]
fn main(args: Args) -> Result<(), Error> {
    let err_path = Path::new(ERRORS_DIR).join(format!("cronjob-{}.log", args.name));
    if !args.no_diskspace_check {
        //TODO move part of diskspace to a library crate and use that instead
        let fs = System::new().mount_at("/")?;
        if fs.avail < ByteSize::gib(5) || (fs.avail.as_u64() as f64 / fs.total.as_u64() as f64) < 0.05
        || fs.files_avail < 5000 || (fs.files_avail as f64 / fs.files_total as f64) < 0.05 {
            writeln!(File::create(err_path)?, "{}\nnot enough disk space", Utc::now().format("%Y-%m-%dT%H:%M:%SZ"))?;
            return Ok(())
        }
    }
    let output = match Command::new(args.cmd).args(args.args).stdout(Stdio::piped()).stderr(Stdio::piped()).output() {
        Ok(output) => output,
        Err(e) => {
            writeln!(File::create(err_path)?, "{}\nerror calling cronjob:\n{}\n{:?}", Utc::now().format("%Y-%m-%dT%H:%M:%SZ"), e, e)?;
            return Ok(())
        }
    };
    if output.status.success() {
        fs::remove_file(err_path).not_found_ok()?;
    } else {
        let mut err_file = File::create(err_path)?;
        write!(err_file, "{}\ncronjob exited with {}:\n\nstdout:\n", Utc::now().format("%Y-%m-%dT%H:%M:%SZ"), output.status)?;
        err_file.write_all(&output.stdout)?;
        write!(err_file, "\nstderr:\n")?;
        err_file.write_all(&output.stderr)?;
        err_file.flush()?;
    }
    Ok(())
}
