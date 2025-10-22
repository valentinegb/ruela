use std::{
    fs::exists,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::ProjectDirs;
use poise::serenity_prelude::{
    self as serenity, CacheHttp, GenericChannelId, GuildId, Message, MessageId,
};
use poise_error::anyhow::{self, anyhow};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub const INVOCABLE_IN_GUILD: &str = "should only be invocable in a guild";

pub trait OrganizationalUnit {
    fn dir(&self) -> anyhow::Result<PathBuf>;

    fn get_data<P: AsRef<Path>, T: DeserializeOwned + Default>(
        &self,
        path: P,
    ) -> anyhow::Result<T> {
        let joined_path = self.dir()?.join(path);

        if exists(&joined_path)? {
            let file = std::fs::File::open(joined_path)?;

            Ok(ciborium::from_reader(file)?)
        } else {
            Ok(Default::default())
        }
    }

    fn set_data<P: AsRef<Path>, T: Serialize>(&self, path: P, data: &T) -> anyhow::Result<()> {
        let joined_path = self.dir()?.join(path);

        if let Some(parent) = joined_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::File::create(joined_path)?;

        ciborium::into_writer(data, file)?;

        Ok(())
    }
}

impl OrganizationalUnit for GuildId {
    fn dir(&self) -> anyhow::Result<PathBuf> {
        Ok(data_dir()?.join("guilds").join(self.get().to_string()))
    }
}

pub trait Data<U: OrganizationalUnit>: DeserializeOwned + Serialize + Default {
    const PATH: &str;

    fn get_data_from(unit: U) -> anyhow::Result<Self> {
        unit.get_data(Self::PATH)
    }

    fn set_data_for(&self, unit: U) -> anyhow::Result<()> {
        unit.set_data(Self::PATH, self)
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct Rules(Vec<Rule>);

impl Data<GuildId> for Rules {
    const PATH: &str = "rules.cbor";
}

impl Deref for Rules {
    type Target = Vec<Rule>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Rules {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Deserialize, Serialize)]
pub struct Rule {
    original: TimestampedText,
    amendments: Vec<TimestampedText>,
    repealed: bool,
}

impl Rule {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            original: TimestampedText::new(text),
            amendments: Vec::new(),
            repealed: false,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct TimestampedText {
    text: String,
    timestamp: u64,
}

impl TimestampedText {
    fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time travel is real")
                .as_secs(),
        }
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct RulesMessage(Option<(GenericChannelId, MessageId)>);

impl Data<GuildId> for RulesMessage {
    const PATH: &str = "rules_message.cbor";
}

impl From<Message> for RulesMessage {
    fn from(value: Message) -> Self {
        Self(Some((value.channel_id, value.id)))
    }
}

impl RulesMessage {
    pub async fn get(
        &self,
        cache_http: impl CacheHttp,
    ) -> Option<Result<Message, serenity::Error>> {
        match self.0 {
            Some((channel_id, message_id)) => {
                Some(channel_id.message(cache_http, message_id).await)
            }
            None => None,
        }
    }
}

fn data_dir() -> anyhow::Result<PathBuf> {
    let project_dirs = ProjectDirs::from("com", "valentinegb", "mod-bot").ok_or(
        anyhow!("no valid home directory path could be retrieved from the operating system")
            .context("could not construct project dirs"),
    )?;

    Ok(project_dirs.data_dir().to_owned())
}

/// This function assumes it is being called from within a guild-only command.
pub fn get_guild_id_from_ctx(ctx: poise_error::Context<'_>) -> GuildId {
    ctx.guild_id().expect(INVOCABLE_IN_GUILD)
}
