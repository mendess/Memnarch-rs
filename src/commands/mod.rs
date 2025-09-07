mod bday;
mod global;
mod moderation;
mod mtg_spoilers;
mod quotes;
pub mod sfx;
mod tts;

type Context<'c> = poise::Context<'c, mappable_rc::Marc<super::Bot>, anyhow::Error>;
type Command = poise::Command<mappable_rc::Marc<super::Bot>, anyhow::Error>;

pub mod command_groups {
    use super::*;

    pub use global::commands as global;

    // 797882422884433940
    pub use self::mtg_spoilers::commands as mtg_spoilers;
    pub use moderation::commands as moderation;

    // 136220994812641280
    pub use bday::bday;
    pub use quotes::commands as quotes;
    pub use sfx::sfx;
    pub use tts::tts;

    pub fn all() -> Vec<Command> {
        global()
            .chain(mtg_spoilers())
            .chain(moderation())
            .chain([bday()])
            .chain(quotes())
            .chain([sfx()])
            .chain([tts()])
            .collect()
    }
}
