#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
        ffi::OsString,
        path::Path,
        process::Stdio,
    },
    bytesize::ByteSize,
    chrono::prelude::*,
    systemstat::{
        Platform as _,
        System,
    },
    tokio::{
        io::AsyncWriteExt as _,
        process::Command,
    },
    wheel::{
        fs::{
            self,
            File,
        },
        traits::IoResultExt as _,
    },
    cron_wrapper::ERRORS_DIR,
};

#[derive(clap::Parser)]
#[clap(version)]
struct Args {
    name: String,
    cmd: OsString,
    args: Vec<OsString>,
    #[clap(long)]
    no_diskspace_check: bool,
}

#[wheel::main]
async fn main(args: Args) -> wheel::Result {
    let err_path = Path::new(ERRORS_DIR).join(format!("cronjob-{}.log", args.name));
    if !args.no_diskspace_check {
        //TODO move part of diskspace to a library crate and use that instead
        let fs = System::new().mount_at("/").at("/")?;
        if fs.avail < ByteSize::gib(5) || (fs.avail.as_u64() as f64 / fs.total.as_u64() as f64) < 0.05
        || fs.files_avail < 5000 || (fs.files_avail as f64 / fs.files_total as f64) < 0.05 {
            let mut err_file = File::create(&err_path).await?;
            err_file.write_all(format!("{}\nnot enough disk space\n", Utc::now().format("%Y-%m-%dT%H:%M:%SZ")).as_ref()).await.at(err_path)?;
            return Ok(())
        }
    }
    let output = match Command::new(args.cmd).args(args.args).stdout(Stdio::piped()).stderr(Stdio::piped()).output().await {
        Ok(output) => output,
        Err(e) => {
            let mut err_file = File::create(&err_path).await?;
            err_file.write_all(format!("{}\nerror calling cronjob:\n{}\n{:?}\n", Utc::now().format("%Y-%m-%dT%H:%M:%SZ"), e, e).as_ref()).await.at(err_path)?;
            return Ok(())
        }
    };
    if output.status.success() {
        fs::remove_file(err_path).await.missing_ok()?;
    } else {
        let mut err_file = File::create(&err_path).await?;
        err_file.write_all(format!("{}\ncronjob exited with {}:\n\nstdout:\n", Utc::now().format("%Y-%m-%dT%H:%M:%SZ"), output.status).as_ref()).await.at(&err_path)?;
        err_file.write_all(&output.stdout).await.at(&err_path)?;
        err_file.write_all(b"\nstderr:\n").await.at(&err_path)?;
        err_file.write_all(&output.stderr).await.at(&err_path)?;
        err_file.flush().await.at(err_path)?;
    }
    Ok(())
}
