use quicli::prelude::*;
use std::collections::HashMap;
use std::convert::From;
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
use serde::ser::{Serialize, Serializer};
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

    #[serde(default, skip_serializing_if = "Application::omit_ser")]
    pub app: Application,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<HashMap<String, Application>>,

    #[serde(default)]
    pub runner: Runner,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectInclude {
    #[serde(default)]
    pub app: Application,

    pub apps: Option<HashMap<String, Application>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner: Option<RunnerInclude>,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    Elixir,
    Rust,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerMode {
    None,

    Docker,

    DockerCompose,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Application {
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<HashMap<String, Config>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Config {
    Map(HashMap<String, ConfigValue>),

    Single(ConfigValue),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigValue(String);

#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Runner {
    pub valid: Vec<RunnerEntry>,

    #[serde(default)]
    pub default: Vec<RunnerEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RunnerInclude {
    pub default: Vec<RunnerEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunnerEntry {
    pub long: String,

    pub short: String,
}

pub enum ConfigSearch {
    Single(Config),

    Multiple(HashMap<String, Config>),
}

#[derive(Debug, Fail)]
enum ProjectError {
    #[fail(display = "Unsupported file format: {:?}", ext)]
    UnsupportedFileFormat { ext: OsString },

    #[fail(display = "The file {:?} already exists", path)]
    ExistingFile { path: OsString },

    #[fail(
        display = "Incompatible config value types (found string and map). Key: {}",
        key
    )]
    IncompatibleConfigType { key: String },

    #[fail(display = "Error while parsing file {:?}: {}", path, error)]
    FileParseError {
        path: PathBuf,
        error: serde_yaml::Error,
    },
}

impl Project {
    pub fn new(name: &str) -> Self {
        Project {
            name: name.to_string(),
            description: Some("An Onyx project".to_string()),
            language: None,
            container: ContainerMode::None,
            umbrella: false,
            include: vec![],
            included: None,
            app: Default::default(),
            apps: None,
            runner: Default::default(),
        }
    }

    pub fn load(path: &PathBuf) -> Result<Self> {
        validate_file(path, "yml")?;
        let content = read_file(path)?;
        let mut project: Project =
            serde_yaml::from_str(&content).map_err(|err| ProjectError::FileParseError {
                path: path.clone(),
                error: err,
            })?;
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
            app: {
                let mut merged = self.app.clone();
                if let Some(ref included) = self.included {
                    for inc in included {
                        merged.merge(&inc.app)?;
                    }
                }

                merged
            },
            apps: {
                let mut apps = self.apps.clone();
                if let Some(ref included) = self.included {
                    for inc in included {
                        if let Some(ref mut apps) = apps {
                            if let Some(ref inc_apps) = inc.apps {
                                for (name, app) in inc_apps {
                                    if apps.contains_key(name) {
                                        apps.get_mut(name).unwrap().merge(app)?;
                                    } else {
                                        apps.insert(name.to_string(), app.clone());
                                    }
                                }
                            }
                        } else {
                            apps = inc.apps.clone();
                        }
                    }
                }

                apps
            },
            runner: {
                let mut runner = Runner {
                    valid: self.runner.valid.clone(),
                    default: {
                        if let Some(ref included) = self.included {
                            let mut reversed = included.clone();
                            reversed.reverse();

                            let last = reversed.iter().find(|inc| inc.runner.is_some());
                            if let Some(ref last) = last {
                                last.runner.as_ref().unwrap().default.clone()
                            } else {
                                self.runner.default.clone()
                            }
                        } else {
                            self.runner.default.clone()
                        }
                    },
                };

                runner.validate_and_normalize()?;

                runner
            },
        })
    }

    pub fn get_config(
        &self,
        _app: &Option<String>,
        _key: &String,
        _sub_key: &Option<String>,
    ) -> Result<ConfigSearch> {
        Ok(ConfigSearch::Single(Config::Single(ConfigValue::from(""))))
    }
}

impl ProjectInclude {
    pub fn load(path: &PathBuf) -> Result<Self> {
        validate_file(path, "yml")?;
        let content = read_file(path)?;
        let include: ProjectInclude =
            serde_yaml::from_str(&content).map_err(|err| ProjectError::FileParseError {
                path: path.clone(),
                error: err,
            })?;

        Ok(include)
    }
}

impl Default for ContainerMode {
    fn default() -> Self {
        ContainerMode::None
    }
}

impl Application {
    fn omit_ser(&self) -> bool {
        self.config.is_none()
    }

    fn merge(&mut self, other: &Application) -> Result<()> {
        self.config = {
            let mut merged = self.config.clone();

            if let Some(ref config) = other.config {
                if let Some(ref mut m) = merged {
                    for (key, val) in config {
                        if m.contains_key(key) {
                            let mut m_val = m[key].clone();
                            m_val.merge(val, key)?;
                            m.insert(key.to_string(), m_val);
                        } else {
                            m.insert(key.to_string(), val.clone());
                        }
                    }
                } else {
                    merged = Some(config.clone());
                }
            }

            merged
        };
        Ok(())
    }
}

impl Config {
    fn merge(&mut self, other: &Config, key: &str) -> Result<()> {
        use self::ProjectError::IncompatibleConfigType;
        use Config::*;

        let merged;

        match other {
            Map(other) => match self {
                Map(map) => {
                    for (key, val) in other {
                        map.insert(key.to_string(), val.clone());
                    }

                    merged = Map(map.clone());
                }
                Single(_) => bail!(IncompatibleConfigType {
                    key: key.to_string()
                }),
            },
            Single(other) => match self {
                Map(_) => bail!(IncompatibleConfigType {
                    key: key.to_string()
                }),
                Single(_) => {
                    merged = Single(other.clone());
                }
            },
        }

        *self = merged;

        Ok(())
    }
}

impl FromStr for ConfigValue {
    // This implementation of `from_str` can never fail, so use the impossible
    // `Void` type as the error type.
    type Err = Void;

    fn from_str(s: &str) -> Res<Self, Self::Err> {
        Ok(ConfigValue(s.to_string()))
    }
}

impl<T: AsRef<str>> From<T> for ConfigValue {
    fn from(t: T) -> Self {
        ConfigValue(t.as_ref().to_string())
    }
}

macro_rules! implement_str_or_num_from {
    ($func: ident, $t:ty) => {

        fn $func<E>(self, value: $t) -> Res<ConfigValue, E>
        where
            E: de::Error,
        {
            Ok(ConfigValue(value.to_string()))
        }
    };
}

impl<'de> Deserialize<'de> for ConfigValue {
    fn deserialize<D>(deserializer: D) -> Res<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ConfigValueVisitor;

        impl<'de> Visitor<'de> for ConfigValueVisitor {
            type Value = ConfigValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("string or number")
            }

            implement_str_or_num_from!(visit_str, &str);
            implement_str_or_num_from!(visit_i8, i8);
            implement_str_or_num_from!(visit_i16, i16);
            implement_str_or_num_from!(visit_i32, i32);
            implement_str_or_num_from!(visit_i64, i64);
            implement_str_or_num_from!(visit_u8, u8);
            implement_str_or_num_from!(visit_u16, u16);
            implement_str_or_num_from!(visit_u32, u32);
            implement_str_or_num_from!(visit_u64, u64);
            implement_str_or_num_from!(visit_f32, f32);
            implement_str_or_num_from!(visit_f64, f64);
            implement_str_or_num_from!(visit_bool, bool);
        }

        deserializer.deserialize_any(ConfigValueVisitor)
    }
}

impl Serialize for ConfigValue {
    fn serialize<S>(&self, serializer: S) -> Res<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Ok(int) = self.0.parse() {
            return serializer.serialize_u64(int);
        }

        if let Ok(int) = self.0.parse() {
            return serializer.serialize_i64(int);
        }

        if let Ok(float) = self.0.parse() {
            return serializer.serialize_f64(float);
        }

        if let Ok(boolean) = self.0.parse() {
            return serializer.serialize_bool(boolean);
        }

        serializer.serialize_str(&self.0)
    }
}

impl Runner {
    fn validate_and_normalize(&mut self) -> Result<()> {
        self.default = self
            .default
            .iter()
            .map(|def| self.validate_entry(def).map(|entry| entry.clone()))
            .collect::<Result<Vec<_>>>()?;

        Ok(())
    }

    fn validate_entry(&self, entry: &RunnerEntry) -> Result<&RunnerEntry> {
        let valid = self.valid.iter().find(|RunnerEntry { long, short }| {
            entry.long.to_lowercase() == *long.to_lowercase()
                || entry.short.to_lowercase() == *short.to_lowercase()
        });

        ensure!(
            valid.is_some(),
            "Invalid default runner entry: {}",
            entry.long
        );

        Ok(valid.unwrap())
    }

    pub fn entries_to_run(&self, args: &[String]) -> Result<Vec<String>> {
        let processed = args
            .iter()
            .map(|arg| {
                let item = if arg.starts_with("+") {
                    ('+', self.validate_entry(&arg[1..].into()))
                } else if arg.starts_with("-") {
                    ('-', self.validate_entry(&arg[1..].into()))
                } else {
                    ('+', self.validate_entry(&arg[..].into()))
                };

                match item {
                    (sign, Ok(arg)) => Ok((sign, arg)),
                    (_sign, Err(error)) => Err(error),
                }
            })
            .collect::<Result<Vec<_>>>()?;

        let to_add: Vec<_> = processed
            .iter()
            .filter(|item| item.0 == '+')
            .map(|(_, arg)| (*arg).clone())
            .collect();

        let to_remove: Vec<_> = processed
            .iter()
            .filter(|item| item.0 == '-')
            .map(|(_, arg)| *arg)
            .collect();

        Ok(self
            .default
            .iter()
            .chain(to_add.iter())
            .filter(|arg| !to_remove.contains(&arg))
            .map(|arg| arg.long.clone())
            .collect())
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

impl<T: AsRef<str>> From<T> for RunnerEntry {
    fn from(t: T) -> Self {
        RunnerEntry {
            long: t.as_ref().to_string(),
            short: t.as_ref().to_string(),
        }
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
            app: Application {
                config: Some(
                    [("key".to_string(), Config::Single("XXXX".into()))]
                        .iter()
                        .cloned()
                        .collect(),
                ),
            },
            apps: if umbrella { Some(HashMap::new()) } else { None },
            runner: Default::default(),
        }
    };

    let mut config = HashMap::new();
    config.insert("key".to_string(), Config::Single("XXXX".into()));
    let db: HashMap<_, _> = [
        ("host", "localhost"),
        ("port", "80"),
        ("user", "user"),
        ("pass", "secret"),
        ("db", "db"),
    ]
    .iter()
    .map(|(key, val)| (key.to_string(), val.into()))
    .collect();
    config.insert("db".to_string(), Config::Map(db));
    project.app.config = Some(config);

    debug!("Generated file: {:#?}", project);
    let serialized = serde_yaml::to_string(&project)?;
    let mut output = File::create(file)?;
    output.write(serialized.as_bytes())?;

    println!("Project file generated successfully");

    if project.include.len() > 0 {
        for file in &project.include {
            validate_file_not_exists(file)?;
            let mut included = ProjectInclude {
                app: Application {
                    config: {
                        let mut db = HashMap::new();
                        db.insert("port".to_string(), "8080".into());
                        let mut config = HashMap::new();
                        config.insert("db".to_string(), Config::Map(db));
                        Some(config)
                    },
                },
                apps: None,
                runner: Default::default(),
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

impl fmt::Display for ConfigSearch {
    fn fmt(&self, f: &mut fmt::Formatter) -> Res<(), fmt::Error> {
        f.write_str("example")
    }
}
