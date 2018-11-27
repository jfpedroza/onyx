use quicli::prelude::*;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::fs::File;
use std::io::ErrorKind;
use std::io::Write;
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,

    #[serde(default)]
    pub container: ContainerMode,

    #[serde(default)]
    pub umbrella: bool,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<PathBuf>,

    #[serde(skip)]
    pub included: Option<Vec<ProjectInclude>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<Application>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<HashMap<String, Application>>,

    #[serde(default)]
    pub runner: Runner,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectInclude {
    pub app: Option<Application>,

    pub apps: Option<HashMap<String, Application>>,

    pub runner: Option<RunnerInclude>,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    Elixir,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Config {
    Map(HashMap<String, StrOrNum>),

    Single(String),
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Runner {
    pub valid: Vec<RunnerEntry>,

    #[serde(default)]
    pub default: Vec<RunnerEntry>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct RunnerInclude {
    pub default: Vec<RunnerEntry>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct RunnerEntry {
    pub long: String,

    pub short: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StrOrNum(String);

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
            description: Some("An Onyx project".to_string()),
            language: None,
            container: ContainerMode::None,
            umbrella: false,
            include: vec![],
            included: None,
            app: Some(Application { config: None }),
            apps: None,
            runner: Default::default(),
        }
    }

    pub fn load(path: &PathBuf) -> Result<Self> {
        validate_file(path, "yml")?;
        let content = read_file(path)?;
        let mut project: Project = serde_yaml::from_str(&content)?;
        project.validate_and_normalize()?;

        project.included = Some(
            project
                .include
                .iter()
                .map(|file| ProjectInclude::load(file))
                .collect::<Result<_>>()?,
        );

        Ok(project)
    }

    fn validate_and_normalize(&mut self) -> Result<()> {
        self.runner.validate_and_normalize()?;
        Ok(())
    }

    pub fn merge(&self) -> Result<Self> {
        Ok(Project {
            name: self.name.clone(),
            description: self.description.clone(),
            language: self.language,
            container: self.container,
            umbrella: self.umbrella,
            include: vec![],
            included: None,
            app: None,
            apps: None,
            runner: Default::default(),
        })
    }
}

impl ProjectInclude {
    pub fn load(path: &PathBuf) -> Result<Self> {
        validate_file(path, "yml")?;
        let content = read_file(path)?;
        let include: ProjectInclude = serde_yaml::from_str(&content)?;

        Ok(include)
    }
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

impl FromStr for StrOrNum {
    // This implementation of `from_str` can never fail, so use the impossible
    // `Void` type as the error type.
    type Err = Void;

    fn from_str(s: &str) -> Res<Self, Self::Err> {
        Ok(StrOrNum(s.to_string()))
    }
}

impl<'de> Deserialize<'de> for StrOrNum {
    fn deserialize<D>(deserializer: D) -> Res<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrOrNumVisitor;

        impl<'de> Visitor<'de> for StrOrNumVisitor {
            type Value = StrOrNum;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("string or number")
            }

            fn visit_str<E>(self, value: &str) -> Res<StrOrNum, E>
            where
                E: de::Error,
            {
                Ok(StrOrNum(value.to_string()))
            }

            fn visit_i32<E>(self, value: i32) -> Res<StrOrNum, E>
            where
                E: de::Error,
            {
                Ok(StrOrNum(value.to_string()))
            }

            fn visit_i64<E>(self, value: i64) -> Res<StrOrNum, E>
            where
                E: de::Error,
            {
                Ok(StrOrNum(value.to_string()))
            }

            fn visit_f32<E>(self, value: f32) -> Res<StrOrNum, E>
            where
                E: de::Error,
            {
                Ok(StrOrNum(value.to_string()))
            }

            fn visit_f64<E>(self, value: f64) -> Res<StrOrNum, E>
            where
                E: de::Error,
            {
                Ok(StrOrNum(value.to_string()))
            }
        }

        deserializer.deserialize_any(StrOrNumVisitor)
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

fn validate_file_not_exists(path: &PathBuf) -> Result<()> {
    use self::ProjectError::*;

    match fs::metadata(path) {
        Ok(_) => {
            bail!(ExistingFile {
                path: path.as_os_str().to_os_string()
            });
        }
        Err(err) => match err.kind() {
            ErrorKind::NotFound => {}
            _ => bail!(err),
        },
    }

    Ok(())
}

pub fn init(file: &PathBuf, name: &Option<String>) -> Result<()> {
    validate_file_not_exists(file)?;

    let mut project = if let Some(name) = name {
        Project::new(&name)
    } else {
        let name: String = prompt("Name of the project");
        let description = prompt_default("Description", "".to_string());
        let umbrella = prompt_default("Umbrella", false);
        let include_file = prompt_default("Generate and include a onyx.priv.yml file?", true);

        Project {
            name,
            description: Some(description),
            language: None,
            container: ContainerMode::None,
            umbrella,
            include: if include_file {
                vec![PathBuf::from("onyx.priv.yml")]
            } else {
                vec![]
            },
            included: None,
            app: Some(Application {
                config: Some(
                    [("key".to_string(), Config::Single("XXXX".to_string()))]
                        .iter()
                        .cloned()
                        .collect(),
                ),
            }),
            apps: if umbrella { Some(HashMap::new()) } else { None },
            runner: Default::default(),
        }
    };

    let mut config = HashMap::new();
    config.insert("key".to_string(), Config::Single("XXXX".to_string()));
    let db: HashMap<_, _> = [
        ("host", "localhost"),
        ("port", "80"),
        ("user", "user"),
        ("pass", "secret"),
        ("db", "db"),
    ]
        .iter()
        .map(|(key, val)| (key.to_string(), val.to_string()))
        .collect();
    config.insert("db".to_string(), Config::Map(db));
    project.app.as_mut().unwrap().config = Some(config);

    debug!("Generated file: {:#?}", project);
    let serialized = serde_yaml::to_string(&project)?;
    let mut output = File::create(file)?;
    output.write(serialized.as_bytes())?;

    println!("Project file generated successfully");

    if project.include.len() > 0 {
        for file in &project.include {
            validate_file_not_exists(file)?;
            let mut included = ProjectInclude {
                app: Some(Application {
                    config: {
                        let mut db = HashMap::new();
                        db.insert("port".to_string(), "8080".to_string());
                        let mut config = HashMap::new();
                        config.insert("db".to_string(), Config::Map(db));
                        Some(config)
                    },
                }),
                apps: None,
                runner: None,
            };

            debug!("Generated include file: {:#?}", included);
            let serialized = serde_yaml::to_string(&included)?;
            let mut output = File::create(file)?;
            output.write(serialized.as_bytes())?;
        }

        println!("Include files generated successfully");
    }

    Ok(())
}
