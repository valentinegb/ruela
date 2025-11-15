use std::{
    fs::exists,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use directories::ProjectDirs;
use poise::serenity_prelude::{GuildId, Member, UserId};
use poise_error::anyhow::{self, Context, anyhow};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub const INVOCABLE_IN_GUILD: &str = "should only be invocable in a guild";
pub const UNIX_EPOCH_ELAPSED_ERR: &str = "time travel is real";

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

impl<T: OrganizationalUnit> OrganizationalUnit for &T {
    fn dir(&self) -> anyhow::Result<PathBuf> {
        T::dir(self)
    }
}

impl OrganizationalUnit for GuildId {
    fn dir(&self) -> anyhow::Result<PathBuf> {
        Ok(data_dir()?.join("guilds").join(self.get().to_string()))
    }
}

impl OrganizationalUnit for Member {
    fn dir(&self) -> anyhow::Result<PathBuf> {
        self.guild_id.dir().map(|guild_dir| {
            guild_dir
                .join("members")
                .join(self.user.id.get().to_string())
        })
    }
}

pub trait Data<U: OrganizationalUnit>: DeserializeOwned + Serialize + Default {
    const PATH: &str;
    /// User-facing descriptor of what this data represents. Used in error messages.
    const DESCRIPTOR: &str;

    fn get_data_from(unit: U) -> anyhow::Result<Self> {
        unit.get_data(Self::PATH)
            .context(format!("could not get {}", Self::DESCRIPTOR))
    }

    fn set_data_for(&self, unit: U) -> anyhow::Result<()> {
        unit.set_data(Self::PATH, self)
            .context(format!("could not set {}", Self::DESCRIPTOR))
    }
}

impl<T: Data<U>, U: OrganizationalUnit> Data<&U> for T {
    const PATH: &'static str = T::PATH;
    const DESCRIPTOR: &'static str = T::DESCRIPTOR;
}

fn data_dir() -> anyhow::Result<PathBuf> {
    let project_dirs = ProjectDirs::from("com", "valentinegb", "ruela").ok_or(
        anyhow!("no valid home directory path could be retrieved from the operating system")
            .context("could not construct project dirs"),
    )?;

    Ok(project_dirs.data_dir().to_owned())
}

#[derive(Deserialize, Serialize, Clone, Copy)]
pub struct Attribution {
    pub user: UserId,
    pub timestamp: u64,
}

impl From<UserId> for Attribution {
    fn from(value: UserId) -> Self {
        Self {
            user: value,
            timestamp: UNIX_EPOCH
                .elapsed()
                .expect(UNIX_EPOCH_ELAPSED_ERR)
                .as_secs(),
        }
    }
}

/// This function assumes it is being called from within a guild-only command.
pub fn get_guild_id_from_ctx(ctx: poise_error::Context<'_>) -> GuildId {
    ctx.guild_id().expect(INVOCABLE_IN_GUILD)
}
