use quicli::prelude::*;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::result::Result as Res;
use std::str::FromStr;

use promptly::{prompt, prompt_default};
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use void::Void;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,

    pub description: Option<String>,

    pub language: Option<Language>,

    #[serde(default)]
    pub container: ContainerMode,

    #[serde(default)]
    pub umbrella: bool,

    #[serde(default)]
    pub include: Vec<PathBuf>,

    pub app: Option<Application>,

    pub apps: Option<HashMap<String, Application>>,

    #[serde(default)]
    pub runner: Runner,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    Elixir,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerMode {
    None,

    Docker,

    DockerCompose,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Application {
    config: Option<HashMap<String, Config>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Config {
    Map(HashMap<String, String>),

    Single(String),
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Runner {
    pub valid: Vec<RunnerEntry>,

    #[serde(default)]
    pub default: Vec<RunnerEntry>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct RunnerEntry {
    pub long: String,

    pub short: String,
}

#[derive(Debug, Fail)]
enum ProjectError {
    #[fail(display = "Unsupported file format: {:?}", ext)]
    UnsupportedFileFormat { ext: OsString },

    #[fail(display = "The file {:?} already exists", path)]
    ExistingFile { path: OsString },
}

impl Project {
    pub fn new(name: &String) -> Self {
        Project {
            name: name.to_string(),
            description: None,
            language: None,
            container: ContainerMode::None,
            umbrella: false,
            include: vec![],
            app: Some(Application { config: None }),
            apps: None,
            runner: Default::default(),
        }
    }

    pub fn load(path: &PathBuf) -> Result<Self> {
        validate_file(path, "yml")?;
        let content = read_file(path)?;
        let mut project_settings: Project = serde_yaml::from_str(&content)?;
        project_settings.validate_and_normalize()?;

        Ok(project_settings)
    }

    fn validate_and_normalize(&mut self) -> Result<()> {
        self.runner.validate_and_normalize()?;
        Ok(())
    }
    //valid_list.contains(def.long) || valid_list.contains(def.short),
}

impl Default for ContainerMode {
    fn default() -> Self {
        ContainerMode::None
    }
}

impl Runner {
    fn validate_and_normalize(&mut self) -> Result<()> {
        for def in &self.default {
            ensure!(
                self.valid
                    .iter()
                    .find(|RunnerEntry { long, short }| def.long == *long || def.short == *short)
                    .is_some(),
                "Invalid default runner entry: {}",
                def.long
            );
        }

        Ok(())
    }
}

impl FromStr for RunnerEntry {
    // This implementation of `from_str` can never fail, so use the impossible
    // `Void` type as the error type.
    type Err = Void;

    fn from_str(s: &str) -> Res<Self, Self::Err> {
        Ok(RunnerEntry {
            long: s.to_string(),
            short: s.to_string(),
        })
    }
}

impl<'de> Deserialize<'de> for RunnerEntry {
    fn deserialize<D>(deserializer: D) -> Res<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Long,
            Short,
        }

        struct StringOrStruct(PhantomData<fn() -> RunnerEntry>);

        impl<'de> Visitor<'de> for StringOrStruct {
            type Value = RunnerEntry;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("string or map")
            }

            fn visit_str<E>(self, value: &str) -> Res<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FromStr::from_str(value).unwrap())
            }

            fn visit_map<M>(self, mut visitor: M) -> Res<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut long = None;
                let mut short = None;
                while let Some(key) = visitor.next_key()? {
                    match key {
                        Field::Long => {
                            if long.is_some() {
                                return Err(de::Error::duplicate_field("long"));
                            }

                            long = Some(visitor.next_value()?)
                        }

                        Field::Short => {
                            if short.is_some() {
                                return Err(de::Error::duplicate_field("short"));
                            }

                            short = Some(visitor.next_value()?)
                        }
                    }
                }

                let long: String = long.ok_or_else(|| de::Error::missing_field("long"))?;
                let short = short.unwrap_or(long.clone());

                Ok(RunnerEntry { long, short })
            }
        }

        deserializer.deserialize_any(StringOrStruct(PhantomData))
    }
}

fn validate_file(path: &PathBuf, extension: &str) -> Result<()> {
    use self::ProjectError::*;

    let metadata = fs::metadata(path)?;
    ensure!(metadata.is_file(), "{:?} is not a file.", path);
    ensure!(
        path.extension().is_some(),
        UnsupportedFileFormat {
            ext: path.file_name().unwrap().to_os_string(),
        }
    );

    let ext = path.extension().unwrap();
    ensure!(
        ext == OsString::from(extension),
        UnsupportedFileFormat {
            ext: OsString::from(ext),
        }
    );

    Ok(())
}

pub fn init(file: &PathBuf, name: &Option<String>) -> Result<()> {
    use self::ProjectError::*;

    match fs::metadata(file) {
        Ok(_) => {
            bail!(ExistingFile {
                path: file.as_os_str().to_os_string()
            });
        }
        Err(err) => match err.kind() {
            ErrorKind::NotFound => {}
            _ => bail!(err),
        },
    }

    let project = if let Some(name) = name {
        Project::new(&name)
    } else {
        let name: String = prompt("Name of the project");
        let description: Option<String> = prompt("Description");
        let umbrella = prompt_default("Umbrella", false);
        let include_file = prompt_default("Generate and include a onyx.private.yml file?", true);

        Project {
            name,
            description,
            language: None,
            container: ContainerMode::None,
            umbrella,
            include: if include_file {
                vec![PathBuf::from("onyx.private.yml")]
            } else {
                vec![]
            },
            app: Some(Application { config: None }),
            apps: None,
            runner: Default::default(),
        }
    };

    println!("Generated file: {:#?}", project);

    Ok(())
}
