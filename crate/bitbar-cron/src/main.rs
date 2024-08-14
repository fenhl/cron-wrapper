use {
    std::{
        convert::Infallible as Never,
        ffi::OsString,
        iter,
        path::Path,
        process::Stdio,
    },
    bitbar::{
        ContentItem,
        Flavor,
        Menu,
        MenuItem,
    },
    futures::stream::{
        self,
        StreamExt as _,
        TryStreamExt as _,
    },
    if_chain::if_chain,
    itertools::Itertools as _,
    lazy_regex::regex_captures,
    serde::Deserialize,
    tokio::process::Command,
    wheel::{
        fs,
        traits::AsyncCommandOutputExt as _,
    },
    cron_wrapper::{
        ERRORS_DIR,
        ERRORS_DIR_LINUX,
    },
};

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

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)] Json(#[from] serde_json::Error),
    #[error(transparent)] Utf8(#[from] std::string::FromUtf8Error),
    #[error(transparent)] Wheel(#[from] wheel::Error),
    #[error(transparent)] Xdg(#[from] xdg::BaseDirectoriesError),
    #[error("invalid UTF-8")]
    OsString(OsString),
}

impl From<OsString> for Error {
    fn from(s: OsString) -> Self {
        Self::OsString(s)
    }
}

impl From<Error> for Menu {
    fn from(e: Error) -> Menu {
        match e {
            Error::Wheel(wheel::Error::CommandExit { name, output }) => Menu(vec![
                MenuItem::new(format!("subcommand {name} exited with {}", output.status)),
                MenuItem::new(format!("stdout: {}", String::from_utf8_lossy(&output.stdout))),
                MenuItem::new(format!("stderr: {}", String::from_utf8_lossy(&output.stderr))),
            ]),
            e => Menu(vec![MenuItem::new(e)]),
        }
    }
}

#[derive(Default, Deserialize)]
struct Config {
    #[serde(default)]
    hosts: Vec<String>,
}

impl Config {
    async fn load() -> Result<Self, Error> {
        let path = xdg::BaseDirectories::new()?.find_config_file("bitbar/plugins/cron.json");
        Ok(if_chain! {
            if let Some(path) = path;
            if fs::exists(&path).await?; //TODO replace with fs::read_json NotFound error handling
            then {
                fs::read_json(path).await?
            } else {
                Self::default()
            }
        })
    }
}

async fn failed_cronjobs_local() -> Result<Vec<String>, Error> {
    fs::read_dir(ERRORS_DIR)
        .filter_map(|entry| async {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => return Some(Err(e.into())),
            };
            let file_name = match entry.file_name().into_string() {
                Ok(file_name) => file_name,
                Err(raw_file_name) => return Some(Err(raw_file_name.into())),
            };
            regex_captures!("^cronjob-(.+)\\.log$", &file_name).map(|(_, name)| Ok(name.to_owned()))
        })
        .try_collect().await
}

async fn failed_cronjobs_ssh(host: &str) -> Result<Vec<String>, Error> {
    let output = Command::new("ssh").arg(host).arg("ls").arg(ERRORS_DIR_LINUX).stdout(Stdio::piped()).check("ssh").await?;
    Ok(
        String::from_utf8(output.stdout)?
            .lines()
            .filter_map(|file_name| regex_captures!("^cronjob-(.+)\\.log$", file_name).map(|(_, name)| name.to_owned()))
            .collect()
    )
}

#[bitbar::main] //TODO error-template-image
async fn main(flavor: Flavor) -> Result<Menu, Error> {
    let config = Config::load().await?;
    let host_groups = stream::once(async { Ok::<_, Error>((format!("localhost"), failed_cronjobs_local().await?)) })
        .chain(stream::iter(config.hosts).then(|host| async {
            let failed_cronjobs = failed_cronjobs_ssh(&host).await?;
            Ok((host, failed_cronjobs))
        }))
        .try_collect::<Vec<_>>().await?;
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
