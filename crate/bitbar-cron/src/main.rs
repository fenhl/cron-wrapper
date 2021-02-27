#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

use {
    std::{
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
        Menu,
        MenuItem,
    },
    derive_more::From,
    itertools::Itertools as _,
    lazy_static::lazy_static,
    regex::Regex,
    serde::Deserialize,
    cron_wrapper::{
        ERRORS_DIR,
        ERRORS_DIR_LINUX,
    },
};

lazy_static! {
    static ref ERROR_LOG_REGEX: Regex = Regex::new("^cronjob-(.+)\\.log$").expect("failed to build error log filename regex");
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
fn main() -> Result<Menu, Error> {
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
        let mut menu = vec![
            MenuItem::new(format!("cron: {}", total)), //TODO replace “cron” text with a template-image
            MenuItem::Sep,
        ];
        menu.extend(host_groups.into_iter()
            .filter_map(|(host, mut failed_cronjobs)| (!failed_cronjobs.is_empty()).then(|| {
                failed_cronjobs.sort();
                Box::new(iter::once(MenuItem::new(host))
                    .chain(failed_cronjobs.into_iter().map(MenuItem::new))) as Box<dyn Iterator<Item = MenuItem>>
            }))
            .intersperse_with(|| Box::new(iter::once(MenuItem::Sep)))
            .flatten());
        Menu(menu)
    })
}
