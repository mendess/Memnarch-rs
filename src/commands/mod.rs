pub mod custom;
pub mod general;
pub mod interrail;
pub mod owner;
pub mod quotes;
pub mod sfx;
pub mod tts;

pub mod command_groups {
    use super::*;
    pub use custom::CUSTOM_GROUP;
    pub use general::GENERAL_GROUP;
    pub use interrail::INTERRAIL_GROUP;
    pub use owner::OWNER_GROUP;
    pub use quotes::QUOTES_GROUP;
    pub use sfx::{SFXALIASES_GROUP, SFX_GROUP};
    pub use tts::TTS_GROUP;
}
