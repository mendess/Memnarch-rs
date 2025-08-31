mod bday;
mod global;
mod moderation;
mod mtg_spoilers;
mod quotes;
pub mod sfx;
mod tts;

type Context<'c> = poise::Context<'c, super::Bot, anyhow::Error>;
type Command = poise::Command<super::Bot, anyhow::Error>;

pub mod command_groups {
    use super::*;

    pub use global::commands as global;

    // 797882422884433940
    pub use moderation::commands as moderation;
    pub use self::mtg_spoilers::commands as mtg_spoilers;

    // 136220994812641280
    pub use quotes::commands as quotes;
    pub use sfx::sfx;
    pub use tts::tts;
    pub use bday::bday;
}
