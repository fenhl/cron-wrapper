#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
        convert::Infallible as Never,
        ffi::OsString,
        fs::File,
        io,
        iter,
        path::Path,
        process::{
            Command,
            Output,
            Stdio,
        },
    },
    bitbar::{
        ContentItem,
        Flavor,
        Menu,
        MenuItem,
    },
    derive_more::From,
    itertools::Itertools as _,
    once_cell::sync::Lazy,
    regex::Regex,
    serde::Deserialize,
    cron_wrapper::{
        ERRORS_DIR,
        ERRORS_DIR_LINUX,
    },
};

static ERROR_LOG_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new("^cronjob-(.+)\\.log$").expect("failed to build error log filename regex"));

trait ResultNeverExt {
    type Ok;

    fn never_unwrap(self) -> Self::Ok;
}

impl<T> ResultNeverExt for Result<T, Never> {
    type Ok = T;

    fn never_unwrap(self) -> T {
        match self {
            Ok(x) => x,
            Err(never) => match never {},
        }
    }
}

#[derive(From)]
enum Error {
    CommandExit(Output),
    ConfigFormat(serde_json::Error),
    Io(io::Error),
    OsString(OsString),
    Utf8(std::string::FromUtf8Error),
}

impl From<Error> for Menu {
    fn from(e: Error) -> Menu {
        match e {
            Error::CommandExit(output) => Menu(vec![
                MenuItem::new(format!("subcommand exited with {}", output.status)),
                MenuItem::new(format!("stdout: {}", String::from_utf8_lossy(&output.stdout))),
                MenuItem::new(format!("stderr: {}", String::from_utf8_lossy(&output.stderr))),
            ]),
            Error::ConfigFormat(e) => Menu(vec![MenuItem::new(format!("error reading config file: {}", e))]),
            Error::Io(e) => Menu(vec![MenuItem::new(format!("I/O error: {}", e))]),
            Error::OsString(_) | Error::Utf8(_) => Menu(vec![MenuItem::new("filename was not valid UTF-8")]),
        }
    }
}

#[derive(Default, Deserialize)]
struct Config {
    #[serde(default)]
    hosts: Vec<String>,
}

impl Config {
    fn new() -> Result<Config, Error> {
        let dirs = xdg_basedir::get_config_home().into_iter().chain(xdg_basedir::get_config_dirs());
        Ok(if let Some(file) = dirs.filter_map(|cfg_dir| File::open(cfg_dir.join("bitbar/plugins/cron.json")).ok()).next() {
            serde_json::from_reader(file).map_err(Error::ConfigFormat)?
        } else {
            Config::default()
        })
    }
}

fn failed_cronjobs_local() -> Result<Vec<String>, Error> {
    Path::new(ERRORS_DIR).read_dir()?
        .filter_map(|entry| {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => return Some(Err(e.into())),
            };
            let file_name = match entry.file_name().into_string() {
                Ok(file_name) => file_name,
                Err(raw_file_name) => return Some(Err(raw_file_name.into())),
            };
            ERROR_LOG_REGEX.captures(&file_name).map(|captures| Ok(captures[1].to_owned()))
        })
        .try_collect()
}

fn failed_cronjobs_ssh(host: &str) -> Result<Vec<String>, Error> {
    let output = Command::new("ssh").arg(host).arg("ls").arg(ERRORS_DIR_LINUX).stdout(Stdio::piped()).output()?;
    if !output.status.success() { return Err(Error::CommandExit(output)) }
    Ok(
        String::from_utf8(output.stdout)?
            .lines()
            .filter_map(|file_name| ERROR_LOG_REGEX.captures(file_name).map(|captures| captures[1].to_owned()))
            .collect()
    )
}

#[bitbar::main] //TODO error-template-image
fn main(flavor: Flavor) -> Result<Menu, Error> {
    let config = Config::new()?;
    let host_groups = iter::once(Ok::<_, Error>((format!("localhost"), failed_cronjobs_local()?)))
        .chain(config.hosts.into_iter().map(|host| {
            let failed_cronjobs = failed_cronjobs_ssh(&host)?;
            Ok((host, failed_cronjobs))
        }))
        .try_collect::<_, Vec<_>, _>()?;
    let total = host_groups.iter()
        .map(|(_, failed_cronjobs)| failed_cronjobs.len())
        .sum::<usize>();
    Ok(if total == 0 {
        Menu::default()
    } else {
        let header = if let Flavor::SwiftBar(swiftbar) = flavor {
            let mut header = ContentItem::new(total);
            swiftbar.sf_image(&mut header, "calendar.badge.clock");
            header.into()
        } else {
            MenuItem::new(format!("cron: {}", total))
        };
        let mut menu = vec![header, MenuItem::Sep];
        #[allow(unstable_name_collisions)] //TODO use std impl of intersperse_with when stabilized
        menu.extend(host_groups.into_iter()
            .filter_map(|(host, mut failed_cronjobs)| (!failed_cronjobs.is_empty()).then(move || {
                failed_cronjobs.sort();
                Box::new(iter::once(MenuItem::new(&host))
                    .chain(failed_cronjobs.into_iter().map(move |cronjob| {
                        let item = ContentItem::new(&cronjob);
                        if host == "localhost" {
                            item.command(("open", Path::new(ERRORS_DIR).join(format!("cronjob-{}.log", cronjob)).display()))
                        } else {
                            item.command(bitbar::attr::Command::terminal(("ssh", &host, "cat", Path::new(ERRORS_DIR_LINUX).join(format!("cronjob-{}.log", cronjob)).display())))
                        }.never_unwrap().into()
                    }))) as Box<dyn Iterator<Item = MenuItem>>
            }))
            .intersperse_with(|| Box::new(iter::once(MenuItem::Sep)))
            .flatten());
        Menu(menu)
    })
}
